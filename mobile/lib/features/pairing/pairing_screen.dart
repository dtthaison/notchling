import 'dart:async';
import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:mobile_scanner/mobile_scanner.dart';

import '../../app/theme/tokens.dart';
import '../../app/widgets/common.dart';
import '../../core/pairing/pairing_payload.dart';
import '../../l10n/app_localizations.dart';
import 'pairing_controller.dart';

/// Pair with a Mac: scan its QR, or type or paste the same data (simulators have no camera).
class PairingScreen extends ConsumerStatefulWidget {
  const PairingScreen({super.key, this.canCancel = false});

  /// Shown when a Mac is already paired and this screen adds another.
  final bool canCancel;

  @override
  ConsumerState<PairingScreen> createState() => _PairingScreenState();
}

class _PairingScreenState extends ConsumerState<PairingScreen> {
  bool _scanning = false;
  final TextEditingController _json = TextEditingController();
  final TextEditingController _host = TextEditingController();
  final TextEditingController _port = TextEditingController(text: '47821');
  final TextEditingController _fingerprint = TextEditingController();
  final TextEditingController _code = TextEditingController();

  @override
  void initState() {
    super.initState();
    if (kDebugMode) unawaited(_loadPrefill());
  }

  /// Debug builds only: a pairing JSON dropped in the app's temp folder fills the paste field, for
  /// simulators and emulators where typing hex through the system keyboard is unreliable.
  Future<void> _loadPrefill() async {
    final File file = File('${Directory.systemTemp.path}/pairing_prefill.json');
    if (!file.existsSync()) return;
    final String text = await file.readAsString();
    if (mounted) _json.text = text;
  }

  @override
  void dispose() {
    for (final TextEditingController c in <TextEditingController>[_json, _host, _port, _fingerprint, _code]) {
      c.dispose();
    }
    super.dispose();
  }

  Future<void> _submitManual() async {
    final PairingController controller = ref.read(pairingControllerProvider.notifier);
    final PairingPayload payload;
    try {
      if (_json.text.trim().isNotEmpty) {
        payload = PairingPayload.parse(_json.text);
      } else {
        payload = PairingPayload(
          name: 'Mac',
          port: int.tryParse(_port.text.trim()) ?? 0,
          fingerprint: _fingerprint.text.trim().toLowerCase(),
          hosts: _host.text.split(',').map((String h) => h.trim()).where((String h) => h.isNotEmpty).toList(),
          code: _code.text.trim().toUpperCase(),
        )..validate();
      }
    } on PairingFormatException {
      controller.fail(PairingFailure.invalid);
      return;
    }
    await _pair(payload);
  }

  Future<void> _pair(PairingPayload payload) async {
    final bool done = await ref.read(pairingControllerProvider.notifier).pair(payload);
    if (done && mounted && widget.canCancel) Navigator.of(context).pop();
  }

  void _onScan(BarcodeCapture capture) {
    if (ref.read(pairingControllerProvider).phase == PairingPhase.waiting) return;
    for (final Barcode code in capture.barcodes) {
      final String? raw = code.rawValue;
      if (raw == null) continue;
      try {
        unawaited(_pair(PairingPayload.parse(raw)));
        return;
      } on PairingFormatException {
        ref.read(pairingControllerProvider.notifier).fail(PairingFailure.invalid);
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    final AppL10n l10n = AppL10n.of(context);
    final PairingUiState ui = ref.watch(pairingControllerProvider);
    final bool waiting = ui.phase == PairingPhase.waiting;

    return Scaffold(
      appBar: AppBar(
        backgroundColor: Token.surface,
        title: Text(l10n.pairTitle, style: TypeToken.titleMd),
        leading: widget.canCancel ? IconButton(onPressed: () => Navigator.of(context).pop(), icon: const Icon(Icons.close), tooltip: l10n.close) : null,
        automaticallyImplyLeading: false,
      ),
      body: SafeArea(
        child: ListView(
          padding: const EdgeInsets.all(Space.lg),
          children: <Widget>[
            Text(l10n.pairIntro, style: TypeToken.bodyMd.copyWith(color: Token.muted)),
            const SizedBox(height: Space.lg),
            Row(children: <Widget>[
              Expanded(child: _ModeButton(label: l10n.pairScan, icon: Icons.qr_code_scanner, selected: _scanning, onTap: () => setState(() => _scanning = true))),
              const SizedBox(width: Space.sm),
              Expanded(child: _ModeButton(label: l10n.pairManual, icon: Icons.keyboard, selected: !_scanning, onTap: () => setState(() => _scanning = false))),
            ]),
            const SizedBox(height: Space.lg),
            if (_scanning)
              ClipRRect(
                borderRadius: BorderRadius.circular(Radii.lg),
                child: SizedBox(
                  height: 320,
                  child: MobileScanner(
                    onDetect: _onScan,
                    errorBuilder: (BuildContext context, MobileScannerException error) => StateMessage(
                      icon: Icons.no_photography,
                      title: l10n.pairCameraDenied,
                      isError: true,
                    ),
                  ),
                ),
              )
            else
              ..._manualFields(l10n, waiting),
            const SizedBox(height: Space.lg),
            if (waiting)
              Row(children: <Widget>[
                const SizedBox(width: 18, height: 18, child: CircularProgressIndicator(strokeWidth: 2)),
                const SizedBox(width: Space.md),
                Expanded(child: Text(l10n.pairWaiting, style: TypeToken.bodyMd)),
              ])
            else if (ui.failure != null)
              Semantics(
                liveRegion: true,
                child: Text(
                  switch (ui.failure!) {
                    PairingFailure.invalid => l10n.pairInvalid,
                    PairingFailure.fingerprintMismatch => l10n.pairFingerprintMismatch,
                    PairingFailure.denied => l10n.pairDenied,
                    PairingFailure.unreachable => l10n.pairUnreachable,
                  },
                  style: TypeToken.bodyMd.copyWith(color: Token.statusError),
                ),
              ),
          ],
        ),
      ),
    );
  }

  List<Widget> _manualFields(AppL10n l10n, bool waiting) => <Widget>[
        _Field(controller: _json, label: l10n.pairPasteJson, lines: 3, mono: true, enabled: !waiting),
        const SizedBox(height: Space.md),
        _Field(controller: _host, label: l10n.pairHost, mono: true, enabled: !waiting, keyboard: TextInputType.url),
        const SizedBox(height: Space.md),
        _Field(controller: _port, label: l10n.pairPort, mono: true, enabled: !waiting, keyboard: TextInputType.number),
        const SizedBox(height: Space.md),
        _Field(controller: _fingerprint, label: l10n.pairFingerprint, mono: true, enabled: !waiting, lines: 2),
        const SizedBox(height: Space.md),
        _Field(controller: _code, label: l10n.pairCode, mono: true, enabled: !waiting, caps: true),
        const SizedBox(height: Space.lg),
        SizedBox(width: double.infinity, child: PrimaryButton(label: l10n.pairConnect, onPressed: waiting ? null : () => unawaited(_submitManual()))),
      ];
}

class _ModeButton extends StatelessWidget {
  const _ModeButton({required this.label, required this.icon, required this.selected, required this.onTap});
  final String label;
  final IconData icon;
  final bool selected;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    return Semantics(
      button: true,
      selected: selected,
      label: label,
      excludeSemantics: true,
      onTap: onTap,
      child: InkWell(
        onTap: onTap,
        borderRadius: BorderRadius.circular(Radii.md),
        child: Container(
          constraints: const BoxConstraints(minHeight: Metric.minTarget),
          padding: const EdgeInsets.symmetric(horizontal: Space.md, vertical: Space.sm),
          decoration: BoxDecoration(
            color: selected ? Token.surfaceRaised : Colors.transparent,
            borderRadius: BorderRadius.circular(Radii.md),
            border: Border.all(color: selected ? Token.primary : Token.border),
          ),
          child: Row(mainAxisAlignment: MainAxisAlignment.center, children: <Widget>[
            Icon(icon, size: 18, color: selected ? Token.primary : Token.muted),
            const SizedBox(width: Space.sm),
            Flexible(child: Text(label, style: TypeToken.labelSm.copyWith(color: selected ? Token.onSurface : Token.muted, fontSize: 13))),
          ]),
        ),
      ),
    );
  }
}

class _Field extends StatelessWidget {
  const _Field({required this.controller, required this.label, this.lines = 1, this.mono = false, this.enabled = true, this.keyboard, this.caps = false});
  final TextEditingController controller;
  final String label;
  final int lines;
  final bool mono;
  final bool enabled;
  final TextInputType? keyboard;
  final bool caps;

  @override
  Widget build(BuildContext context) {
    return TextField(
      controller: controller,
      enabled: enabled,
      minLines: lines,
      maxLines: lines,
      keyboardType: keyboard,
      autocorrect: false,
      enableSuggestions: false,
      textCapitalization: caps ? TextCapitalization.characters : TextCapitalization.none,
      style: (mono ? TypeToken.codeSm : TypeToken.bodyMd).copyWith(color: Token.onSurface),
      decoration: InputDecoration(labelText: label, labelStyle: TypeToken.bodySm),
    );
  }
}
