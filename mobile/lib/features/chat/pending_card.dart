import 'package:flutter/material.dart';

import '../../app/theme/tokens.dart';
import '../../app/widgets/common.dart';
import '../../core/protocol/models.dart';
import '../../l10n/app_localizations.dart';

/// The question or permission the Mac is waiting on, above the composer. Permissions have Cho phép /
/// Từ chối; questions list their options as buttons. Multi-select questions are answered on the Mac.
class PendingCard extends StatefulWidget {
  const PendingCard({super.key, required this.prompt, required this.onAllow, required this.onChoose, this.error});

  final PendingPrompt prompt;
  final String? error;
  final void Function({required bool allow}) onAllow;

  /// One 1-based option number per question, sent once the last question is answered.
  final void Function(List<int> choices) onChoose;

  @override
  State<PendingCard> createState() => _PendingCardState();
}

class _PendingCardState extends State<PendingCard> {
  final List<int> _picked = <int>[];

  void _pick(int option) {
    final List<int> next = <int>[..._picked, option];
    if (next.length == widget.prompt.questions.length) {
      _picked.clear();
      widget.onChoose(next);
    } else {
      setState(() => _picked.add(option));
    }
  }

  @override
  Widget build(BuildContext context) {
    final AppL10n l10n = AppL10n.of(context);
    final PendingPrompt prompt = widget.prompt;
    final List<Widget> body = <Widget>[];

    if (prompt.isPermission) {
      body
        ..add(Text(prompt.summary, maxLines: 4, overflow: TextOverflow.ellipsis, style: TypeToken.codeSm.copyWith(color: Token.onSurface)))
        ..add(const SizedBox(height: Space.md))
        ..add(Wrap(spacing: Space.sm, runSpacing: Space.sm, children: <Widget>[
          PrimaryButton(label: l10n.allow, semanticLabel: '${l10n.allow} ${prompt.toolName}', onPressed: () => widget.onAllow(allow: true)),
          SecondaryButton(label: l10n.deny, semanticLabel: '${l10n.deny} ${prompt.toolName}', onPressed: () => widget.onAllow(allow: false)),
        ]));
    } else if (prompt.hasMultiSelect || prompt.questions.isEmpty) {
      body
        ..add(Text(l10n.multiSelectOnMac, style: TypeToken.bodyMd))
        ..add(const SizedBox(height: Space.sm))
        ..add(Text(prompt.summary, style: TypeToken.bodySm));
    } else {
      final PromptQuestion question = prompt.questions[_picked.length.clamp(0, prompt.questions.length - 1)];
      body
        ..add(if_(question.header.isNotEmpty, Text(question.header, style: TypeToken.labelSm)))
        ..add(Text(question.question, style: TypeToken.bodyMd))
        ..add(const SizedBox(height: Space.md))
        ..add(Wrap(spacing: Space.sm, runSpacing: Space.sm, children: <Widget>[
          for (int i = 0; i < question.options.length; i++)
            SecondaryButton(label: '${i + 1}  ${question.options[i]}', semanticLabel: '${i + 1}: ${question.options[i]}', onPressed: () => _pick(i + 1)),
        ]));
    }

    return Semantics(
      container: true,
      label: prompt.isPermission ? l10n.pendingPermission(prompt.toolName) : l10n.pendingQuestion,
      child: Container(
        margin: const EdgeInsets.fromLTRB(Space.md, Space.sm, Space.md, 0),
        decoration: BoxDecoration(
          color: Token.surfaceRaised,
          borderRadius: BorderRadius.circular(Radii.lg),
          border: Border.all(color: Token.border),
        ),
        clipBehavior: Clip.antiAlias,
        child: IntrinsicHeight(
          child: Row(crossAxisAlignment: CrossAxisAlignment.stretch, children: <Widget>[
            Container(width: 2, color: Token.statusInput),
            Expanded(
              child: Padding(
                padding: const EdgeInsets.all(Space.md),
                child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: <Widget>[
                  Row(children: <Widget>[
                    const Icon(Icons.mark_unread_chat_alt, size: 14, color: Token.statusInput),
                    const SizedBox(width: Space.sm),
                    Expanded(
                      child: Text(prompt.isPermission ? l10n.pendingPermission(prompt.toolName) : l10n.pendingQuestion,
                          style: TypeToken.labelSm.copyWith(color: Token.statusInput)),
                    ),
                  ]),
                  const SizedBox(height: Space.sm),
                  ...body,
                  if (widget.error != null) ...<Widget>[
                    const SizedBox(height: Space.sm),
                    Text(l10n.answerFailed, style: TypeToken.bodySm.copyWith(color: Token.statusError)),
                  ],
                ]),
              ),
            ),
          ]),
        ),
      ),
    );
  }
}

Widget if_(bool condition, Widget child) => condition ? child : const SizedBox.shrink();
