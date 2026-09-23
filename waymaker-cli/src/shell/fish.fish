# waymaker shell integration for fish
function __wm_pwd_change --on-variable PWD
    wm add "$PWD" >/dev/null 2>&1 &
end

function z
    if test (count $argv) -eq 0
        cd ~
    else if test (count $argv) -eq 1 -a -d "$argv[1]"
        cd "$argv[1]"
    else
        set -l target (wm list --dirs $argv | head -n 1)
        if test -n "$target"
            if test -f "$target"
                set target (dirname "$target")
            end
            cd "$target"
        else
            echo "wm: no matching directory found" >&2
            return 1
        end
    end
end

function zi
    set -l target (wm list | wm --frecency)
    if test -n "$target"
        cd "$target"
    end
end
