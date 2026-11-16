### The phone: the Machines popup.

# Title of the popup the phone opens when no primary machine can be reached.
machines-title = Choose a Machine

# Shown under the title of the Machines popup.
machines-intro = Your main machine can't be reached. Pick the machine that should take over.

# Shown in the Machines popup when no other machine is online.
machines-empty = No other machine is online.

# One machine in the Machines popup that holds a copy of the sessions.
# $time is the clock time of the copy, such as 14:02.
machines-resume = Can resume old sessions · copy from { $time }

# One machine in the Machines popup that holds no copy of the sessions.
machines-jobs-only = Controls jobs only

# One machine in the Machines popup that is not allowed to be the primary. It can't be picked.
machines-not-master = Can Be Master is off

# One machine in the Machines popup that is not ready to take over.
machines-not-ready = Not ready to take over

# Button in an inbox card that opens the Machines popup.
machines-choose = Choose a Machine

# Title of the question before a session moves to a machine.
# $machine is the machine's name, such as m1.
machines-confirm-title = Move to { $machine } now?

# Body of that question for a machine with a copy.
# $time is the clock time of the copy, such as 14:02.
# $old is the name of the machine that can't be reached, such as m5.
machines-confirm-body = Sessions continue there from { $time }. Work { $old } did after that is kept on { $old }.

# Body of that question for a machine with a copy when the old machine's name is not known.
# $time is the clock time of the copy, such as 14:02.
machines-confirm-body-unnamed = Sessions continue there from { $time }. Work the old machine did after that is kept on it.

# Body of that question for a machine that holds no copy of the sessions.
# $machine is the machine's name, such as m3.
machines-confirm-body-jobs = { $machine } holds no copy of your sessions, so it will only control jobs. Work done on the old machine stays there.

# Button that sends the move.
machines-move = Move

# Button that sends the move after the risk was shown.
machines-move-anyway = Move Anyway

# Title of the risk question that follows a move the Mac wants confirmed.
machines-risk-title = Move anyway?

# Shown after a move was sent.
# $machine is the machine's name, such as m1.
machines-moving = Moving to { $machine }. Sessions continue there soon.

# Shown when the Mac says the old machine can be reached and nothing moved but gave no words.
machines-relayed-fallback = The old machine is reachable through another machine; nothing moved.

# Shown when a move could not be started.
machines-failed = Couldn't start the move. Try again.
