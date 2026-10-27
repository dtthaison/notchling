import AppKit
import AVFoundation
import ImageIO
import UniformTypeIdentifiers

// encode <framesDir> <out.gif> <out.mp4> <fps> <gifWidth>
let args = CommandLine.arguments
let dir = args[1], gifPath = args[2], mp4Path = args[3]
let fps = Double(args[4]) ?? 12
let gifWidth = Int(args[5]) ?? 960

let files = (try FileManager.default.contentsOfDirectory(atPath: dir)).filter { $0.hasSuffix(".png") }.sorted()
func load(_ name: String) -> CGImage? {
    guard let src = CGImageSourceCreateWithURL(URL(fileURLWithPath: "\(dir)/\(name)") as CFURL, nil) else { return nil }
    return CGImageSourceCreateImageAtIndex(src, 0, nil)
}
func scaled(_ image: CGImage, width: Int) -> CGImage? {
    let height = image.height * width / image.width
    guard let ctx = CGContext(data: nil, width: width, height: height, bitsPerComponent: 8, bytesPerRow: 0,
                              space: CGColorSpaceCreateDeviceRGB(),
                              bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue) else { return nil }
    ctx.interpolationQuality = .high
    ctx.draw(image, in: CGRect(x: 0, y: 0, width: width, height: height))
    return ctx.makeImage()
}

// GIF
if let gif = CGImageDestinationCreateWithURL(URL(fileURLWithPath: gifPath) as CFURL, UTType.gif.identifier as CFString,
                                             files.count, nil) {
    CGImageDestinationSetProperties(gif, [kCGImagePropertyGIFDictionary: [kCGImagePropertyGIFLoopCount: 0]] as CFDictionary)
    let frame = [kCGImagePropertyGIFDictionary: [kCGImagePropertyGIFDelayTime: 1 / fps,
                                                 kCGImagePropertyGIFUnclampedDelayTime: 1 / fps]] as CFDictionary
    for name in files {
        autoreleasepool {
            if let image = load(name), let small = scaled(image, width: gifWidth) {
                CGImageDestinationAddImage(gif, small, frame)
            }
        }
    }
    print(CGImageDestinationFinalize(gif) ? "gif ok" : "gif failed")
}

// MP4
try? FileManager.default.removeItem(atPath: mp4Path)
guard let first = load(files[0]) else { exit(1) }
let size = (w: first.width, h: first.height)
let writer = try AVAssetWriter(outputURL: URL(fileURLWithPath: mp4Path), fileType: .mp4)
let input = AVAssetWriterInput(mediaType: .video, outputSettings: [
    AVVideoCodecKey: AVVideoCodecType.h264, AVVideoWidthKey: size.w, AVVideoHeightKey: size.h,
    AVVideoCompressionPropertiesKey: [AVVideoAverageBitRateKey: 2_500_000],
])
let adaptor = AVAssetWriterInputPixelBufferAdaptor(assetWriterInput: input, sourcePixelBufferAttributes: [
    kCVPixelBufferPixelFormatTypeKey as String: kCVPixelFormatType_32ARGB,
    kCVPixelBufferWidthKey as String: size.w, kCVPixelBufferHeightKey as String: size.h,
])
writer.add(input)
writer.startWriting()
writer.startSession(atSourceTime: .zero)
for (i, name) in files.enumerated() {
    autoreleasepool {
        guard let image = load(name), let pool = adaptor.pixelBufferPool else { return }
        var buffer: CVPixelBuffer?
        CVPixelBufferPoolCreatePixelBuffer(nil, pool, &buffer)
        guard let buffer else { return }
        CVPixelBufferLockBaseAddress(buffer, [])
        let ctx = CGContext(data: CVPixelBufferGetBaseAddress(buffer), width: size.w, height: size.h, bitsPerComponent: 8,
                            bytesPerRow: CVPixelBufferGetBytesPerRow(buffer), space: CGColorSpaceCreateDeviceRGB(),
                            bitmapInfo: CGImageAlphaInfo.noneSkipFirst.rawValue)
        ctx?.draw(image, in: CGRect(x: 0, y: 0, width: size.w, height: size.h))
        CVPixelBufferUnlockBaseAddress(buffer, [])
        while !input.isReadyForMoreMediaData { usleep(2000) }
        adaptor.append(buffer, withPresentationTime: CMTime(value: CMTimeValue(i), timescale: CMTimeScale(fps)))
    }
}
input.markAsFinished()
let done = DispatchSemaphore(value: 0)
writer.finishWriting { done.signal() }
done.wait()
print(writer.status == .completed ? "mp4 ok" : "mp4 failed: \(String(describing: writer.error))")
