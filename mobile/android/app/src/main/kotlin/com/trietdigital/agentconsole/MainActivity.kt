package com.trietdigital.agentconsole

import android.app.NotificationManager
import android.content.Context
import android.content.Intent
import android.media.AudioAttributes
import android.media.AudioManager
import android.media.MediaPlayer
import android.os.Build
import android.view.Display
import android.view.View
import io.flutter.embedding.android.FlutterActivity
import io.flutter.embedding.engine.FlutterEngine
import io.flutter.plugin.common.MethodChannel

class MainActivity : FlutterActivity() {
    override fun configureFlutterEngine(flutterEngine: FlutterEngine) {
        super.configureFlutterEngine(flutterEngine)
        MethodChannel(flutterEngine.dartExecutor.binaryMessenger, "local.agentconsole/sounds").setMethodCallHandler { call, result ->
            if (call.method == "play") {
                playSound(call.argument<String>("name"))
                result.success(null)
            } else {
                result.notImplemented()
            }
        }
        MethodChannel(flutterEngine.dartExecutor.binaryMessenger, "local.agentconsole/keepalive").setMethodCallHandler { call, result ->
            when (call.method) {
                "start" -> {
                    val intent = Intent(this, KeepAliveService::class.java).putExtra(KeepAliveService.EXTRA_TEXT, call.argument<String>("text"))
                    try {
                        // Starting from the foreground (the app is open when the setting changes) is allowed;
                        // updates while backgrounded reuse the running service.
                        startForegroundService(intent)
                        result.success(null)
                    } catch (error: Exception) {
                        result.error("keepalive", error.message, null)
                    }
                }
                "stop" -> {
                    stopService(Intent(this, KeepAliveService::class.java))
                    result.success(null)
                }
                else -> result.notImplemented()
            }
        }
    }

    /// A short in-app sound ("needs_you" or "new_message" from res/raw). Silent when the ringer is not normal or Do Not
    /// Disturb is on, and never louder than a notification.
    private fun playSound(name: String?) {
        if (name == null) return
        val audio = getSystemService(Context.AUDIO_SERVICE) as AudioManager
        if (audio.ringerMode != AudioManager.RINGER_MODE_NORMAL) return
        val notifications = getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.M && notifications.currentInterruptionFilter != NotificationManager.INTERRUPTION_FILTER_ALL) return
        val resource = resources.getIdentifier(name, "raw", packageName)
        if (resource == 0) return
        try {
            val player = MediaPlayer()
            player.setAudioAttributes(
                AudioAttributes.Builder().setUsage(AudioAttributes.USAGE_NOTIFICATION).setContentType(AudioAttributes.CONTENT_TYPE_SONIFICATION).build(),
            )
            resources.openRawResourceFd(resource).use { player.setDataSource(it.fileDescriptor, it.startOffset, it.length) }
            player.setVolume(0.7f, 0.7f)
            player.setOnCompletionListener { it.release() }
            player.prepare()
            player.start()
        } catch (error: Exception) {
            // A sound that cannot play is not worth an error: the banner and the notification already say it.
        }
    }

    override fun onDestroy() {
        // The Dart side (and the connection) goes with the activity, so a leftover "watching" notification would lie.
        if (isFinishing) stopService(Intent(this, KeepAliveService::class.java))
        super.onDestroy()
    }

    override fun onResume() {
        super.onResume()
        requestHighestRefreshRate()
        dropImeFrameSync()
    }

    /// Flutter's text input plugin follows the keyboard animation frame by frame (a WindowInsetsAnimation callback
    /// that defers the insets), and while it does the app is held to about 30 frames a second. The chat doesn't move
    /// with the keyboard frame by frame anyway: it slides on its own once it knows the keyboard's height. Without the
    /// callback that height arrives at the start of the animation, in one step, and the app keeps its full rate.
    private fun dropImeFrameSync() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.R) return
        val flutterView = findViewById<View>(FlutterActivity.FLUTTER_VIEW_ID) ?: return
        flutterView.setWindowInsetsAnimationCallback(null)
        flutterView.setOnApplyWindowInsetsListener(null)
        flutterView.requestApplyInsets()
    }

    /// Phones default to 60 Hz for apps that do not ask; this picks the fastest mode at the current resolution.
    private fun requestHighestRefreshRate() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.M) return
        @Suppress("DEPRECATION")
        val display: Display = windowManager.defaultDisplay ?: return
        val current = display.mode
        val best = display.supportedModes
            .filter { it.physicalWidth == current.physicalWidth && it.physicalHeight == current.physicalHeight }
            .maxByOrNull { it.refreshRate } ?: return
        val attributes = window.attributes
        if (attributes.preferredDisplayModeId == best.modeId) return
        attributes.preferredDisplayModeId = best.modeId
        window.attributes = attributes
    }
}
