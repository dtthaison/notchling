package com.trietdigital.agentconsole

import android.content.Intent
import android.os.Build
import android.view.Display
import io.flutter.embedding.android.FlutterActivity
import io.flutter.embedding.engine.FlutterEngine
import io.flutter.plugin.common.MethodChannel

class MainActivity : FlutterActivity() {
    override fun configureFlutterEngine(flutterEngine: FlutterEngine) {
        super.configureFlutterEngine(flutterEngine)
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

    override fun onDestroy() {
        // The Dart side (and the connection) goes with the activity, so a leftover "watching" notification would lie.
        if (isFinishing) stopService(Intent(this, KeepAliveService::class.java))
        super.onDestroy()
    }

    override fun onResume() {
        super.onResume()
        requestHighestRefreshRate()
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
