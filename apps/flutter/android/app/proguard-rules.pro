# ffmpeg_kit (pulled in by whisper_ggml) is reached from native code; R8 cannot see that.
-keep class com.antonkarpenko.ffmpegkit.** { *; }
-keep class com.arthenica.ffmpegkit.** { *; }
-keep class com.arthenica.smartexception.** { *; }
