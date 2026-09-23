# waymaker shell integration for bash
wm_prompt_command() {
    wm add "$PWD" >/dev/null 2>&1
}
mm_prompt_command() { wm_prompt_command; }
if [[ ";$PROMPT_COMMAND;" != *";wm_prompt_command;"* ]]; then
    PROMPT_COMMAND="wm_prompt_command;${PROMPT_COMMAND:-}"
fi

z() {
    if [ "$#" -eq 0 ]; then
        cd ~ || return
    elif [ "$#" -eq 1 ] && [ -d "$1" ]; then
        cd "$1" || return
    else
        local dir
        dir="$(wm list --dirs "$@" | head -n 1)"
        if [ -n "$dir" ]; then
            dir="${dir/#\~/$HOME}"
            dir="$(realpath "$dir" 2>/dev/null || readlink -f "$dir" 2>/dev/null || echo "$dir")"
            if [ -f "$dir" ]; then
                dir="$(dirname "$dir")"
            fi
            cd "$dir" || return
        else
            dir="$(wm -o jump "$@")"
            if [ -n "$dir" ]; then
                dir="${dir/#\~/$HOME}"
                dir="$(realpath "$dir" 2>/dev/null || readlink -f "$dir" 2>/dev/null || echo "$dir")"
                if [ -f "$dir" ]; then
                    dir="$(dirname "$dir")"
                fi
                cd "$dir" || return
            fi
        fi
    fi
}

zi() {
    local dir
    dir="$(wm -o jump "$@")"
    if [ -n "$dir" ]; then
        dir="${dir/#\~/$HOME}"
        dir="$(realpath "$dir" 2>/dev/null || readlink -f "$dir" 2>/dev/null || echo "$dir")"
        cd "$dir" || return
    fi
}
