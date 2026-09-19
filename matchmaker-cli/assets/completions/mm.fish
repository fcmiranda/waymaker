complete -c mm -l config -r -F
complete -c mm -s o -l override -r -F
complete -c mm -l group-prefix -d 'Specify a prefix that indicates a line is a group header' -r
complete -c mm -s f -l filter -d 'Filter input headlessly matching query, print results to stdout, and exit without entering the TUI' -r
complete -c mm -l download -d 'Download presets from GitHub. Optionally specify a subfolder' -r
complete -c mm -s d -l doc -d 'Display documentation' -r -f -a "options\t''
binds\t''
template\t''
performance\t''
frecency\t''
jump\t''
other\t''"
complete -c mm -l pos -d 'Initial cursor position (0-based index or negative for from-the-end)' -r
complete -c mm -l media -d 'Enable native terminal media previews (images, videos, PDFs) using ratatui-image and set properties. Examples: --media --media size:s --media size:256 type:kitty --media size:xl' -r
complete -c mm -l media-size -d 'Override the pixel resolution limit for media downscaling (images, videos, PDFs). Examples: --media-size 1280, --media-size 800, --media-size xl, --media-size full, --media-size 0' -r
complete -c mm -l color -d 'Colourise the UI with fzf-style key:value pairs (comma-separated). Example: --color border:#cba6f7,hl-fg:#a6e3a1,nav:#89b4fa Keys: fg, bg, hl-fg, hl-bg, border, label, preview-border, preview-label, list-border, list-label, input-border, input-label, header-border, header-label, nav, selected-fg, selected-bg, selected-prefix, unselected-prefix, spinner, yank, cut, symlink' -r
complete -c mm -l nav -d 'Enable navigation mode and set properties. Examples: --nav --nav bar blink:slow --nav bar:plain action-bar color:#a6e3a1 marker:\'>\' bold --nav action-bar:double' -r
complete -c mm -l nav-bind -d 'Navigation-mode key bindings in the form "char:action". Example: --nav-bind \'h:ChDir(..)\' --nav-bind \'l:ChDir({=});Reload\'' -r
complete -c mm -l dump-config
complete -c mm -s F
complete -c mm -l test-keys
complete -c mm -l last-key
complete -c mm -l no-read -d 'Force the default command to run'
complete -c mm -s q -d 'Reduce the verbosity level'
complete -c mm -s v -d 'Increase the verbosity level'
complete -c mm -l sort -d 'Sort input lines alphabetically before injecting into the picker'
complete -c mm -l frecency -d 'Enable frecency tracking and re-ranking for search results'
complete -c mm -l icons -d 'Prepend a Nerd Font file-type icon before each result row'
complete -c mm -l symlink-target -d 'Append symlink target path after the first column when the entry is a symlink'
complete -c mm -l nav-hints -d 'Show keybinding hints in footer/status when Results pane is focused in navigation mode'
complete -c mm -l parent-peek -d 'Show a 3rd pane on the left displaying parent directory contents'
complete -c mm -l status-inline -d 'Display match status counter inline on the right side of the filter input bar'
complete -c mm -s h -l help -d 'Print help'
