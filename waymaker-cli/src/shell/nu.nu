# waymaker shell integration for nushell
export-env {
    $env.config = ($env.config? | default {} | merge {
        hooks: {
            env_change: {
                PWD: [
                    {|before, after| wm add $after }
                ]
            }
        }
    })
}

def --env z [...rest: string] {
    if ($rest | is-empty) {
        cd ~
    } else if ($rest | length) == 1 and ($rest.0 | path exists) {
        cd $rest.0
    } else {
        let target = (wm list --dirs ($rest | str join " ") | lines | first)
        if ($target | is-not-empty) {
            let final_target = if ($target | path type) == "file" { $target | path dirname } else { $target }
            cd $final_target
        }
    }
}
