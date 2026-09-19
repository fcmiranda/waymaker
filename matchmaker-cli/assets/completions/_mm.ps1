
using namespace System.Management.Automation
using namespace System.Management.Automation.Language

Register-ArgumentCompleter -Native -CommandName 'mm' -ScriptBlock {
    param($wordToComplete, $commandAst, $cursorPosition)

    $commandElements = $commandAst.CommandElements
    $command = @(
        'mm'
        for ($i = 1; $i -lt $commandElements.Count; $i++) {
            $element = $commandElements[$i]
            if ($element -isnot [StringConstantExpressionAst] -or
                $element.StringConstantType -ne [StringConstantType]::BareWord -or
                $element.Value.StartsWith('-') -or
                $element.Value -eq $wordToComplete) {
                break
        }
        $element.Value
    }) -join ';'

    $completions = @(switch ($command) {
        'mm' {
            [CompletionResult]::new('--config', '--config', [CompletionResultType]::ParameterName, 'config')
            [CompletionResult]::new('-o', '-o', [CompletionResultType]::ParameterName, 'o')
            [CompletionResult]::new('--override', '--override', [CompletionResultType]::ParameterName, 'override')
            [CompletionResult]::new('--group-prefix', '--group-prefix', [CompletionResultType]::ParameterName, 'Specify a prefix that indicates a line is a group header')
            [CompletionResult]::new('-f', '-f', [CompletionResultType]::ParameterName, 'Filter input headlessly matching query, print results to stdout, and exit without entering the TUI')
            [CompletionResult]::new('--filter', '--filter', [CompletionResultType]::ParameterName, 'Filter input headlessly matching query, print results to stdout, and exit without entering the TUI')
            [CompletionResult]::new('--download', '--download', [CompletionResultType]::ParameterName, 'Download presets from GitHub. Optionally specify a subfolder')
            [CompletionResult]::new('-d', '-d', [CompletionResultType]::ParameterName, 'Display documentation')
            [CompletionResult]::new('--doc', '--doc', [CompletionResultType]::ParameterName, 'Display documentation')
            [CompletionResult]::new('--pos', '--pos', [CompletionResultType]::ParameterName, 'Initial cursor position (0-based index or negative for from-the-end)')
            [CompletionResult]::new('--media', '--media', [CompletionResultType]::ParameterName, 'Enable native terminal media previews (images, videos, PDFs) using ratatui-image and set properties. Examples: --media --media size:s --media size:256 type:kitty --media size:xl')
            [CompletionResult]::new('--media-size', '--media-size', [CompletionResultType]::ParameterName, 'Override the pixel resolution limit for media downscaling (images, videos, PDFs). Examples: --media-size 1280, --media-size 800, --media-size xl, --media-size full, --media-size 0')
            [CompletionResult]::new('--color', '--color', [CompletionResultType]::ParameterName, 'Colourise the UI with fzf-style key:value pairs (comma-separated). Example: --color border:#cba6f7,hl-fg:#a6e3a1,nav:#89b4fa Keys: fg, bg, hl-fg, hl-bg, border, label, preview-border, preview-label, list-border, list-label, input-border, input-label, header-border, header-label, nav, selected-fg, selected-bg, selected-prefix, unselected-prefix, spinner, yank, cut, symlink')
            [CompletionResult]::new('--nav', '--nav', [CompletionResultType]::ParameterName, 'Enable navigation mode and set properties. Examples: --nav --nav bar blink:slow --nav bar:plain action-bar color:#a6e3a1 marker:''>'' bold --nav action-bar:double')
            [CompletionResult]::new('--nav-bind', '--nav-bind', [CompletionResultType]::ParameterName, 'Navigation-mode key bindings in the form "char:action". Example: --nav-bind ''h:ChDir(..)'' --nav-bind ''l:ChDir({=});Reload''')
            [CompletionResult]::new('--dump-config', '--dump-config', [CompletionResultType]::ParameterName, 'dump-config')
            [CompletionResult]::new('-F', '-F ', [CompletionResultType]::ParameterName, 'F')
            [CompletionResult]::new('--test-keys', '--test-keys', [CompletionResultType]::ParameterName, 'test-keys')
            [CompletionResult]::new('--last-key', '--last-key', [CompletionResultType]::ParameterName, 'last-key')
            [CompletionResult]::new('--no-read', '--no-read', [CompletionResultType]::ParameterName, 'Force the default command to run')
            [CompletionResult]::new('-q', '-q', [CompletionResultType]::ParameterName, 'Reduce the verbosity level')
            [CompletionResult]::new('-v', '-v', [CompletionResultType]::ParameterName, 'Increase the verbosity level')
            [CompletionResult]::new('--sort', '--sort', [CompletionResultType]::ParameterName, 'Sort input lines alphabetically before injecting into the picker')
            [CompletionResult]::new('--frecency', '--frecency', [CompletionResultType]::ParameterName, 'Enable frecency tracking and re-ranking for search results')
            [CompletionResult]::new('--icons', '--icons', [CompletionResultType]::ParameterName, 'Prepend a Nerd Font file-type icon before each result row')
            [CompletionResult]::new('--symlink-target', '--symlink-target', [CompletionResultType]::ParameterName, 'Append symlink target path after the first column when the entry is a symlink')
            [CompletionResult]::new('--nav-hints', '--nav-hints', [CompletionResultType]::ParameterName, 'Show keybinding hints in footer/status when Results pane is focused in navigation mode')
            [CompletionResult]::new('--parent-peek', '--parent-peek', [CompletionResultType]::ParameterName, 'Show a 3rd pane on the left displaying parent directory contents')
            [CompletionResult]::new('--status-inline', '--status-inline', [CompletionResultType]::ParameterName, 'Display match status counter inline on the right side of the filter input bar')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
    })

    $completions.Where{ $_.CompletionText -like "$wordToComplete*" } |
        Sort-Object -Property ListItemText
}
