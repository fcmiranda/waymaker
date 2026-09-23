#!/bin/bash
export RUST_LOG=trace
ITEMS_SCRIPT="/home/fecavmi/.dotfiles/main/tmux/.config/tmux/window-picker-items.sh"
eval "$ITEMS_SCRIPT | wm 'start.cmd=$ITEMS_SCRIPT' start.sort=false start.ansi=true start.reload_interval=1000 2> /home/fecavmi/dev/github/matchmaker/fecavmi/mm-trace.log"
