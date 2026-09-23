# waymaker shell integration for zsh
wm_chpwd() {
    wm add "$PWD" >/dev/null 2>&1 &!
}
autoload -U add-zsh-hook
add-zsh-hook chpwd wm_chpwd
mm_chpwd() { wm_chpwd "$@"; }

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

# Context-aware ZLE widget: Object-First ergonomics and canonical path resolution
_wm_jump_widget() {
    zle -I 2>/dev/null || true
    local initial_buf="$BUFFER"
    local raw_result
    raw_result=$(wm --no-read -o jump)
    [[ -z "$raw_result" ]] && { zle reset-prompt; return 0; }

    local -a lines=("${(@f)raw_result}")
    local -a valid_lines=()
    for l in "${lines[@]}"; do
        [[ -n "$l" ]] && valid_lines+=("$l")
    done

    (( ${#valid_lines} == 0 )) && { zle reset-prompt; return 0; }

    # If single directory selected on empty prompt -> cd immediately
    if (( ${#valid_lines} == 1 )) && [[ -z "${initial_buf// /}" ]]; then
        local target="${valid_lines[1]}"
        target="${target/#\~/$HOME}"
        target=$(realpath "$target" 2>/dev/null || echo "$target")
        if [[ -d "$target" ]]; then
            cd "$target" || cd "${valid_lines[1]}"
            BUFFER=""
            zle reset-prompt
            return 0
        fi
    fi

    # Format paths:
    # - If inside $PWD: use shortest relative path (e.g. "completion.md" or "docs/shell/completion.md")
    # - If outside $PWD: use canonical path with ~ compression (e.g. "~/.dotfiles/...")
    local -a formatted_items=()
    for line in "${valid_lines[@]}"; do
        local full_path
        full_path=$(realpath "$line" 2>/dev/null || echo "$line")
        local formatted=""
        if [[ "$full_path" == "$PWD/"* ]]; then
            local rel="${full_path#$PWD/}"
            formatted="${(q-)rel}"
        elif [[ "$full_path" == "$PWD" ]]; then
            formatted="."
        elif [[ "$full_path" == "$HOME"* ]]; then
            local rest="${full_path#$HOME/}"
            if [[ "$rest" != "$full_path" ]]; then
                rest="${(q-)rest}"
                formatted="~/$rest"
            else
                formatted="~"
            fi
        else
            formatted="${(q-)full_path}"
        fi
        formatted_items+=("$formatted")
    done

    local formatted_result="${(j: :)formatted_items}"
    [[ -z "$formatted_result" ]] && { zle reset-prompt; return 0; }

    if [[ -z "${initial_buf// /}" ]]; then
        # Empty command buffer: leading space and cursor at index 0 (Object-First ergonomics)
        BUFFER=" $formatted_result"
        CURSOR=0
    else
        # Active command buffer: append to cursor position with trailing space
        if [[ "$LBUFFER" == *" " || -z "$LBUFFER" ]]; then
            LBUFFER+="$formatted_result "
        else
            LBUFFER+=" $formatted_result "
        fi
    fi

    zle reset-prompt
}
zle -N _wm_jump_widget
_mm_jump_widget() { _wm_jump_widget "$@"; }
zle -N _mm_jump_widget 2>/dev/null || true
