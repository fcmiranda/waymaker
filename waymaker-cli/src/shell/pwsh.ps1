# waymaker shell integration for powershell
function Set-Location-With-Wm {
    param($Path)
    Set-Location $Path
    wm add (Get-Location).Path | Out-Null
}
function Set-Location-With-Mm {
    param($Path)
    Set-Location-With-Wm $Path
}

function z {
    param([string]$Path)
    if ([string]::IsNullOrEmpty($Path)) {
        Set-Location ~
    } elseif (Test-Path -Path $Path -PathType Container) {
        Set-Location $Path
    } else {
        $target = (wm list --dirs $Path | Select-Object -First 1)
        if ($target) {
            if (Test-Path -Path $target -PathType Leaf) {
                $target = Split-Path -Path $target -Parent
            }
            Set-Location $target
        }
    }
}
