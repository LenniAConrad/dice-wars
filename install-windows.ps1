<#
.SYNOPSIS
Installs Dice Wars on Windows from a downloaded copy of this repository, on a
PC that has nothing set up yet.

.DESCRIPTION
Installs Rust (through the official rustup) if it is missing, builds Dice
Wars, installs it with a Start Menu shortcut and an entry in Settings > Apps,
and checks it. Start it by double-clicking install-windows.cmd, which runs
this file without changing PowerShell's script policy. To run it from
PowerShell:

    powershell -ExecutionPolicy Bypass -File install-windows.ps1

No administrator permission is needed, and nothing outside your user profile
is changed.

Why the GNU toolchain: Rust's usual Windows toolchain (MSVC) links with the
Microsoft C++ Build Tools, a download of several GB that needs administrator
permission. Rust's GNU toolchain for Windows (x86_64-pc-windows-gnu) ships its
own linker and the Windows import libraries the game uses (kernel32, user32,
gdi32, winmm, bcrypt), so it builds Dice Wars with nothing else installed.
When the Microsoft C++ build tools and a Windows SDK are already present, the
MSVC toolchain is used instead, as most Rust users on Windows expect.

.PARAMETER Prefix
Folder to install into. The default is %LOCALAPPDATA%\Programs\DiceWars.

.PARAMETER Jobs
Number of parallel build workers. The default is 1 (or CARGO_BUILD_JOBS), which
keeps the PC responsive while the game compiles.

.PARAMETER Uninstall
Remove what this installer put under the prefix, the Start Menu shortcut and
the Settings > Apps entry, instead of installing.

.PARAMETER Help
Show this help.

This file prints the same kind of text as install-linux.sh and install-macos.sh
(numbered steps, "doing", "ok", "note", "warn" and "error" lines, and a closing
SUCCESS or FAILED block). Keep the wording and the five step titles in step
with them. Keep this file ASCII-only: Windows PowerShell 5.1 reads a file
without a byte-order mark as ANSI.
#>
[CmdletBinding()]
param(
    [string]$Prefix = '',
    [int]$Jobs = 0,
    [switch]$Uninstall,
    [switch]$Help
)

Set-StrictMode -Version 2.0
$ErrorActionPreference = 'Stop'
# Windows PowerShell 5.1 downloads very slowly while it draws a progress bar.
$ProgressPreference = 'SilentlyContinue'

$script:TotalSteps = 5
$script:Step = 0
$script:StepTitle = ''
$script:ErrorShown = $false
$script:ExitCode = 1
$script:InstalledAt = ''
$script:Started = Get-Date
$script:WorkDir = ''
$script:StopMarker = 'DICE_WARS_INSTALLER_STOP'

$AppName = 'Dice Wars'
$UninstallKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\DiceWars'
# Everything this installer puts in the prefix folder.
$InstallerFiles = @('DiceWars.exe', 'DiceWars.ico', 'uninstall.ps1')

# What to type to run this installer again, and to undo it, in the messages.
$script:RerunCommand = 'install-windows.cmd'
foreach ($key in $PSBoundParameters.Keys) {
    $value = $PSBoundParameters[$key]
    if ($value -is [System.Management.Automation.SwitchParameter]) {
        if ($value.IsPresent) { $script:RerunCommand += " -$key" }
    } else {
        $script:RerunCommand += " -$key `"$value`""
    }
}
$script:UninstallCommand = 'install-windows.cmd -Uninstall'
if ($Prefix) { $script:UninstallCommand += " -Prefix `"$Prefix`"" }

function Get-Elapsed {
    $total = [int]((Get-Date) - $script:Started).TotalSeconds
    if ($total -ge 60) { return ('{0}m {1:00}s' -f [int][math]::Floor($total / 60), ($total % 60)) }
    return ('{0}s' -f $total)
}
function Write-Step([string]$Title) {
    $script:Step++
    $script:StepTitle = $Title
    Write-Host ''
    Write-Host ('[{0}/{1}] {2}' -f $script:Step, $script:TotalSteps, $Title)
}
function Write-Doing([string]$Text) { Write-Host "      doing: $Text" }
function Write-Ok([string]$Text) { Write-Host "      ok:    $Text" }
function Write-Note([string]$Text) { Write-Host "      note:  $Text" }
function Write-Warn([string]$Text) { Write-Host "      warn:  $Text" }

# Fail CODE MESSAGE [HINT...]: report why the installer stopped, with what the
# user can do about it. The handler at the bottom prints the FAILED block.
function Fail([int]$Code, [string]$Message, [string[]]$Hints = @()) {
    Write-Host "      error: $Message"
    foreach ($hint in $Hints) { Write-Host "      hint:  $hint" }
    $script:ErrorShown = $true
    $script:ExitCode = $Code
    throw $script:StopMarker
}

# Runs a program, showing its output; returns its exit code.
function Invoke-Tool([string]$Path, [string[]]$Arguments = @()) {
    $previous = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        & $Path @Arguments | Out-Host
        return $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $previous
    }
}

# Runs a program and returns its exit code and text output (errors discarded).
function Get-ToolOutput([string]$Path, [string[]]$Arguments = @()) {
    $previous = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    $output = $null
    $code = 1
    try {
        $output = & $Path @Arguments 2>$null
        $code = $LASTEXITCODE
    } catch {
        $code = 1
    } finally {
        $ErrorActionPreference = $previous
    }
    return [pscustomobject]@{ Code = $code; Text = (($output | Out-String).Trim()) }
}

# Downloads over HTTPS to a file, so a failed transfer is never executed.
function Save-Download([string]$Url, [string]$Path) {
    [Net.ServicePointManager]::SecurityProtocol =
        [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
    Invoke-WebRequest -Uri $Url -OutFile $Path -UseBasicParsing
}

function Get-FreeMegabytes([string]$Path) {
    $letter = [System.IO.Path]::GetPathRoot($Path).Substring(0, 1)
    $drive = Get-PSDrive -Name $letter -ErrorAction SilentlyContinue
    if ($drive -and $null -ne $drive.Free) { return [int]($drive.Free / 1MB) }
    return $null
}

# The first Visual Studio or Build Tools install that has the x64 C++ tools.
function Find-BuildTools {
    $programFiles = ${env:ProgramFiles(x86)}
    if (-not $programFiles) { return $null }
    $vswhere = Join-Path $programFiles 'Microsoft Visual Studio\Installer\vswhere.exe'
    if (-not (Test-Path -LiteralPath $vswhere)) { return $null }
    $found = Get-ToolOutput $vswhere @('-products', '*', '-latest', '-requires',
        'Microsoft.VisualStudio.Component.VC.Tools.x86.x64', '-property', 'installationPath')
    if ($found.Code -eq 0 -and $found.Text) { return (($found.Text -split "`r?`n")[0]) }
    return $null
}

# True when a Windows SDK with the x64 libraries Rust links against is present.
function Test-WindowsSdk {
    $programFiles = ${env:ProgramFiles(x86)}
    if (-not $programFiles) { return $false }
    $libraries = Join-Path $programFiles 'Windows Kits\10\Lib'
    if (-not (Test-Path -LiteralPath $libraries)) { return $false }
    foreach ($version in (Get-ChildItem -LiteralPath $libraries -Directory -ErrorAction SilentlyContinue)) {
        if (Test-Path -LiteralPath (Join-Path $version.FullName 'um\x64\kernel32.lib')) { return $true }
    }
    return $false
}

# The rustc version a toolchain reports, as [major, minor], or $null.
function Get-RustVersion([string]$Rustup, [string]$Toolchain) {
    $result = Get-ToolOutput $Rustup @('run', $Toolchain, 'rustc', '--version')
    if ($result.Code -ne 0) { return $null }
    $match = [regex]::Match($result.Text, '^rustc (\d+)\.(\d+)\.')
    if (-not $match.Success) { return $null }
    return @([int]$match.Groups[1].Value, [int]$match.Groups[2].Value)
}

function Test-RustVersion($Version, [int]$NeedMajor, [int]$NeedMinor) {
    if ($null -eq $Version) { return $false }
    return ($Version[0] -gt $NeedMajor -or ($Version[0] -eq $NeedMajor -and $Version[1] -ge $NeedMinor))
}

function Show-Usage {
    Write-Host 'Usage: install-windows.cmd [-Prefix PATH] [-Jobs N]'
    Write-Host '       install-windows.cmd -Uninstall [-Prefix PATH]'
    Write-Host ''
    Write-Host 'Install Dice Wars on this Windows PC, getting whatever is missing first:'
    Write-Host 'Rust (through the official rustup, into %USERPROFILE%\.cargo; it uses'
    Write-Host 'Rust''s GNU toolchain, which brings its own linker, so the multi-GB'
    Write-Host 'Microsoft C++ build tools are not needed). It then builds the game and'
    Write-Host 'installs it to %LOCALAPPDATA%\Programs\DiceWars\DiceWars.exe with a Start'
    Write-Host 'Menu shortcut and an entry in Settings > Apps. No administrator permission'
    Write-Host 'is needed, nothing needs installing first, and it is safe to run again.'
    Write-Host ''
    Write-Host 'Options:'
    Write-Host '  -Prefix PATH   Install into PATH instead'
    Write-Host '  -Jobs N        Build with N parallel workers (default 1, or CARGO_BUILD_JOBS)'
    Write-Host '  -Uninstall     Remove what this installer put under the prefix'
    Write-Host '  -Help          Show this help'
}

try {
    if ($Help) { Show-Usage; exit 0 }

    if ($Uninstall) {
        $script:TotalSteps = 1
        Write-Host 'Dice Wars uninstaller for Windows'
        Write-Step 'Remove Dice Wars'
        if ([Environment]::OSVersion.Platform -ne 'Win32NT') {
            Fail 2 "this installer is for Windows; this computer is not running Windows." `
                @('On a Mac run: bash install-macos.sh', 'On Linux run: bash install-linux.sh')
        }
        if (-not $Prefix) { $Prefix = Join-Path $env:LOCALAPPDATA 'Programs\DiceWars' }
        $Prefix = [System.IO.Path]::GetFullPath($Prefix)
        $shortcut = Join-Path ([Environment]::GetFolderPath('Programs')) "$AppName.lnk"
        Write-Doing "removing what the installer put in $Prefix"
        $removed = 0
        foreach ($name in $InstallerFiles) {
            $path = Join-Path $Prefix $name
            if (Test-Path -LiteralPath $path) {
                try { Remove-Item -LiteralPath $path -Force } catch {
                    Fail 1 "could not remove $path ($($_.Exception.Message))" `
                        @('Close Dice Wars if it is running, then run the same command again.')
                }
                Write-Ok "removed $path"
                $removed++
            }
        }
        if (Test-Path -LiteralPath $shortcut) {
            Remove-Item -LiteralPath $shortcut -Force
            Write-Ok "removed the Start Menu shortcut $shortcut"
            $removed++
        }
        if (Test-Path -LiteralPath $UninstallKey) {
            Remove-Item -LiteralPath $UninstallKey -Recurse -Force
            Write-Ok 'removed the Settings > Apps entry'
            $removed++
        }
        if ($removed -eq 0) {
            Fail 1 "nothing was removed: no Dice Wars files were found in $Prefix." `
                @('Pass the same -Prefix you installed with; the default is %LOCALAPPDATA%\Programs\DiceWars.')
        }
        if ((Test-Path -LiteralPath $Prefix) -and -not (Get-ChildItem -LiteralPath $Prefix -Force)) {
            Remove-Item -LiteralPath $Prefix -Force -ErrorAction SilentlyContinue
        }
        Write-Host ''
        Write-Host "SUCCESS: Dice Wars was removed from $Prefix."
        Write-Host "  Your settings, bookmarks and replays in $(Join-Path $env:APPDATA 'dice-wars') are kept;"
        Write-Host '  delete that folder too to remove them.'
        Write-Host '  The Rust that the installer added is left in place.'
        Write-Host ("  Finished in {0}." -f (Get-Elapsed))
        exit 0
    }

    $GameDir = $PSScriptRoot
    foreach ($needed in @('Cargo.toml', 'Cargo.lock', 'src\main.rs', 'assets\icon.ico')) {
        if (-not $GameDir -or -not (Test-Path -LiteralPath (Join-Path $GameDir $needed))) {
            Write-Host "install-windows.ps1: $needed is missing next to this script."
            Write-Host 'Download all of Dice Wars from https://github.com/LenniAConrad/dice-wars (Code, then Download ZIP),'
            Write-Host 'extract it (right-click, Extract All), and run install-windows.cmd from inside the extracted folder.'
            $script:ErrorShown = $true
            $script:ExitCode = 2
            $script:Step = -1
            throw $script:StopMarker
        }
    }
    $manifest = Get-Content -LiteralPath (Join-Path $GameDir 'Cargo.toml') -Raw
    $minimum = [regex]::Match($manifest, '(?m)^rust-version\s*=\s*"(\d+)\.(\d+)')
    $NeedMajor = 1
    $NeedMinor = 82
    if ($minimum.Success) {
        $NeedMajor = [int]$minimum.Groups[1].Value
        $NeedMinor = [int]$minimum.Groups[2].Value
    }
    $versionMatch = [regex]::Match($manifest, '(?m)^version\s*=\s*"([^"]+)"')
    $GameVersion = '1.0.0'
    if ($versionMatch.Success) { $GameVersion = $versionMatch.Groups[1].Value }
    if ($Jobs -eq 0) {
        $Jobs = 1
        if ($env:CARGO_BUILD_JOBS) {
            $parsed = 0
            if (-not [int]::TryParse($env:CARGO_BUILD_JOBS, [ref]$parsed) -or $parsed -lt 1) {
                Write-Host "install-windows.ps1: CARGO_BUILD_JOBS must be a whole number of at least 1, not: $env:CARGO_BUILD_JOBS"
                $script:ErrorShown = $true
                $script:ExitCode = 2
                $script:Step = -1
                throw $script:StopMarker
            }
            $Jobs = $parsed
        }
    } elseif ($Jobs -lt 1) {
        Write-Host "install-windows.ps1: -Jobs must be a whole number of at least 1, not: $Jobs"
        $script:ErrorShown = $true
        $script:ExitCode = 2
        $script:Step = -1
        throw $script:StopMarker
    }

    Write-Host 'Dice Wars installer for Windows'
    Write-Host 'This will check this PC, install Rust if it is missing, build Dice Wars, and'
    Write-Host 'install it with a Start Menu shortcut. Nothing needs installing first.'

    # ------------------------------------------------------------ 1. computer
    Write-Step 'Check this computer'
    if ([Environment]::OSVersion.Platform -ne 'Win32NT') {
        Fail 2 "this installer is for Windows; this computer is not running Windows." `
            @('On a Mac run: bash install-macos.sh', 'On Linux run: bash install-linux.sh')
    }
    $windows = [Environment]::OSVersion.Version
    if ($windows.Major -lt 10) {
        Fail 2 "Windows 10 or newer is needed; this PC runs Windows $windows." `
            @('Update Windows, then run the same command again.')
    }
    $architecture = $env:PROCESSOR_ARCHITEW6432
    if (-not $architecture) { $architecture = $env:PROCESSOR_ARCHITECTURE }
    if ($architecture -ne 'AMD64') {
        Fail 2 "unsupported processor: $architecture." `
            @('This installer builds Dice Wars for 64-bit Intel and AMD (x64) Windows.')
    }
    if ($PSVersionTable.PSVersion.Major -lt 5) {
        Fail 2 "Windows PowerShell 5 or newer is needed; this is version $($PSVersionTable.PSVersion)." `
            @('Update Windows PowerShell, then run the same command again.')
    }
    Write-Ok "Windows $windows, $architecture"
    if (-not $Prefix) { $Prefix = Join-Path $env:LOCALAPPDATA 'Programs\DiceWars' }
    $Prefix = [System.IO.Path]::GetFullPath($Prefix)
    $shortcut = Join-Path ([Environment]::GetFolderPath('Programs')) "$AppName.lnk"
    Write-Ok "install folder: $Prefix"
    $free = Get-FreeMegabytes $env:USERPROFILE
    if ($null -ne $free) {
        if ($free -lt 1024) {
            Fail 2 "only $free MB are free where Rust is installed ($env:USERPROFILE); about 1024 MB are needed." `
                @('Free some disk space, then run the same command again.')
        }
        Write-Ok "free disk space: $free MB"
    }
    $script:WorkDir = Join-Path ([System.IO.Path]::GetTempPath()) ('dice-wars-install-' + [guid]::NewGuid().ToString('N').Substring(0, 8))
    New-Item -ItemType Directory -Force -Path $script:WorkDir | Out-Null

    # --------------------------------------------------------------- 2. tools
    # The GNU toolchain needs no separate build tools (see the header), so this
    # step only decides which toolchain to use.
    Write-Step 'Install build tools'
    $buildTools = Find-BuildTools
    if ($buildTools -and (Test-WindowsSdk)) {
        $HostTriple = 'x86_64-pc-windows-msvc'
        Write-Ok "Microsoft C++ build tools are installed ($buildTools); building with them"
    } else {
        $HostTriple = 'x86_64-pc-windows-gnu'
        Write-Ok "nothing to install: Rust's GNU toolchain for Windows brings its own linker"
        Write-Note 'the Microsoft C++ build tools (several GB) are not needed'
    }
    $toolchain = "stable-$HostTriple"

    # ----------------------------------------------------------------- 3. rust
    Write-Step 'Install Rust'
    $cargoHome = $env:CARGO_HOME
    if (-not $cargoHome) { $cargoHome = Join-Path $env:USERPROFILE '.cargo' }
    $env:CARGO_HOME = $cargoHome
    $env:PATH = (Join-Path $cargoHome 'bin') + ';' + $env:PATH
    $rustup = Get-Command rustup.exe -ErrorAction SilentlyContinue
    if ($rustup) {
        $rustupPath = $rustup.Source
        Write-Ok "rustup is already installed: $rustupPath"
    } else {
        $initPath = Join-Path $script:WorkDir 'rustup-init.exe'
        Write-Doing "downloading the official Rust installer (rustup) for $HostTriple"
        try { Save-Download "https://static.rust-lang.org/rustup/dist/$HostTriple/rustup-init.exe" $initPath } catch {
            Fail 1 "could not download rustup ($($_.Exception.Message))." `
                @('Check your internet connection (and any proxy or firewall), then run the same command again.')
        }
        Write-Doing "installing rustup and stable Rust into $cargoHome (a download of a few hundred MB; your PATH is not changed)"
        $code = Invoke-Tool $initPath @('-y', '--no-modify-path', '--profile', 'minimal',
            '--default-host', $HostTriple, '--default-toolchain', 'stable')
        if ($code -ne 0) {
            Fail 1 "the rustup installer failed with exit code $code; its messages are above." `
                @('Run the same command again; it is safe to repeat.')
        }
        $rustupPath = Join-Path $cargoHome 'bin\rustup.exe'
        if (-not (Test-Path -LiteralPath $rustupPath)) {
            Fail 1 "rustup installed but is not usable ($rustupPath is missing)."
        }
    }
    $version = Get-RustVersion $rustupPath $toolchain
    if (Test-RustVersion $version $NeedMajor $NeedMinor) {
        Write-Ok "Rust $toolchain is installed; nothing to download"
    } else {
        Write-Doing "installing Rust $toolchain (a download of a few hundred MB)"
        $code = Invoke-Tool $rustupPath @('toolchain', 'install', $toolchain, '--profile', 'minimal', '--no-self-update')
        if ($code -ne 0) {
            Fail 1 "could not install Rust $toolchain (exit code $code)." `
                @('Check your internet connection (static.rust-lang.org must be reachable), then run the same command again.')
        }
        $version = Get-RustVersion $rustupPath $toolchain
        if (-not (Test-RustVersion $version $NeedMajor $NeedMinor)) {
            Fail 1 "Rust $toolchain is still older than $NeedMajor.$NeedMinor, which Dice Wars needs." `
                @("Run: rustup update $toolchain, then run the same command again.")
        }
    }
    # The rustup proxies pick the toolchain from RUSTUP_TOOLCHAIN.
    $env:RUSTUP_TOOLCHAIN = $toolchain
    $cargo = Join-Path (Split-Path -Parent $rustupPath) 'cargo.exe'
    if (-not (Test-Path -LiteralPath $cargo)) { Fail 1 "rustup is installed but $cargo is missing." }
    $rustcHost = ((Get-ToolOutput $rustupPath @('run', $toolchain, 'rustc', '-vV')).Text -split "`r?`n" |
        Where-Object { $_ -like 'host: *' } | Select-Object -First 1)
    if (-not $rustcHost -or $rustcHost.Substring(6).Trim() -ne $HostTriple) {
        Fail 1 "rustc does not build for $HostTriple."
    }
    Write-Ok ("Rust {0}.{1} ready ({2}): {3}" -f $version[0], $version[1], $toolchain, $cargo)

    # ---------------------------------------------------------------- 4. build
    Write-Step 'Build and install Dice Wars'
    # An inherited target would put the build somewhere this script does not look.
    Remove-Item -LiteralPath 'Env:CARGO_BUILD_TARGET' -ErrorAction SilentlyContinue
    if ($Jobs -eq 1) {
        Write-Doing 'building the game with 1 build worker, so the PC stays responsive (usually 1 to 3 minutes)'
    } else {
        Write-Doing "building the game with $Jobs build workers"
    }
    Push-Location -LiteralPath $GameDir
    try {
        $code = Invoke-Tool $cargo @('build', '--release', '--locked', '-j', "$Jobs")
    } finally {
        Pop-Location
    }
    if ($code -ne 0) {
        Fail 1 "the build did not finish (exit code $code); its messages are above." `
            @('Run the same command again; if it fails the same way, report it at https://github.com/LenniAConrad/dice-wars/issues with those messages.')
    }
    $targetDir = $env:CARGO_TARGET_DIR
    if (-not $targetDir) { $targetDir = Join-Path $GameDir 'target' }
    elseif (-not [System.IO.Path]::IsPathRooted($targetDir)) { $targetDir = Join-Path $GameDir $targetDir }
    $built = Join-Path $targetDir 'release\dicegame.exe'
    if (-not (Test-Path -LiteralPath $built)) { Fail 1 "the build finished but $built does not exist." }
    Write-Ok "built $built"

    $game = Join-Path $Prefix 'DiceWars.exe'
    $icon = Join-Path $Prefix 'DiceWars.ico'
    $uninstallScript = Join-Path $Prefix 'uninstall.ps1'
    try {
        New-Item -ItemType Directory -Force -Path $Prefix | Out-Null
        Copy-Item -LiteralPath $built -Destination $game -Force
        Copy-Item -LiteralPath (Join-Path $GameDir 'assets\icon.ico') -Destination $icon -Force
        # A copy of this script, so Settings > Apps can uninstall even after the
        # downloaded folder is deleted.
        Copy-Item -LiteralPath $PSCommandPath -Destination $uninstallScript -Force
        Unblock-File -LiteralPath $uninstallScript -ErrorAction SilentlyContinue
    } catch {
        Fail 1 "could not copy the game to $Prefix ($($_.Exception.Message))." `
            @('Close Dice Wars if it is running, then run the same command again.')
    }
    $script:InstalledAt = $game
    Write-Ok "installed $game"

    try {
        $shell = New-Object -ComObject WScript.Shell
        $link = $shell.CreateShortcut($shortcut)
        $link.TargetPath = $game
        $link.WorkingDirectory = $Prefix
        $link.IconLocation = "$icon,0"
        $link.Description = 'Pastel dice conquest'
        $link.Save()
    } catch {
        Fail 1 "could not create the Start Menu shortcut $shortcut ($($_.Exception.Message))."
    }
    Write-Ok "Start Menu shortcut: $shortcut"

    try {
        $sizeKb = [int](((Get-ChildItem -LiteralPath $Prefix -File | Measure-Object -Property Length -Sum).Sum) / 1KB)
        $uninstallString = "powershell.exe -NoProfile -ExecutionPolicy Bypass -File `"$uninstallScript`" -Uninstall -Prefix `"$Prefix`""
        New-Item -Path $UninstallKey -Force | Out-Null
        $strings = [ordered]@{
            DisplayName          = $AppName
            DisplayVersion       = $GameVersion
            Publisher            = 'LenniAConrad'
            DisplayIcon          = $icon
            InstallLocation      = $Prefix
            UninstallString      = $uninstallString
            QuietUninstallString = $uninstallString
            URLInfoAbout         = 'https://github.com/LenniAConrad/dice-wars'
        }
        foreach ($name in $strings.Keys) {
            New-ItemProperty -Path $UninstallKey -Name $name -Value $strings[$name] -PropertyType String -Force | Out-Null
        }
        foreach ($name in @('NoModify', 'NoRepair')) {
            New-ItemProperty -Path $UninstallKey -Name $name -Value 1 -PropertyType DWord -Force | Out-Null
        }
        New-ItemProperty -Path $UninstallKey -Name 'EstimatedSize' -Value $sizeKb -PropertyType DWord -Force | Out-Null
    } catch {
        Fail 1 "could not add Dice Wars to Settings > Apps ($($_.Exception.Message))."
    }
    Write-Ok 'added Dice Wars to Settings > Apps (for uninstalling)'

    # ---------------------------------------------------------------- 5. verify
    Write-Step 'Check the installed game'
    if (-not (Test-Path -LiteralPath $game)) { Fail 1 "the game file is missing after the install: $game" }
    $stream = [System.IO.File]::OpenRead($game)
    try {
        $header = New-Object byte[] 4096
        $read = $stream.Read($header, 0, $header.Length)
    } finally {
        $stream.Dispose()
    }
    $peOffset = 0
    if ($read -ge 64) { $peOffset = [BitConverter]::ToInt32($header, 60) }
    if ($read -lt 64 -or $header[0] -ne 0x4D -or $header[1] -ne 0x5A -or
        $peOffset -lt 64 -or $peOffset + 6 -gt $read -or
        [BitConverter]::ToUInt16($header, $peOffset + 4) -ne 0x8664) {
        Fail 1 "$game is not a 64-bit Windows program."
    }
    Write-Ok 'the game is a 64-bit Windows program'
    if (-not (Test-Path -LiteralPath $shortcut)) { Fail 1 "the Start Menu shortcut is missing: $shortcut" }
    Write-Ok 'the Start Menu shortcut is in place'
    if (-not (Test-Path -LiteralPath $UninstallKey)) { Fail 1 'the Settings > Apps entry is missing.' }
    Write-Ok 'the Settings > Apps entry is in place'

    Write-Host ''
    Write-Host 'SUCCESS: Dice Wars is installed and passed its checks.'
    Write-Host "  Game file:    $game"
    Write-Host '  Start it:     from the Start Menu (type Dice Wars), or run the game file.'
    Write-Host "  Uninstall:    Settings > Apps > Dice Wars, or: $script:UninstallCommand"
    Write-Host ("  Finished in {0}." -f (Get-Elapsed))
    exit 0
} catch {
    if ($_.Exception.Message -ne $script:StopMarker) {
        Write-Host "      error: this step stopped unexpectedly: $($_.Exception.Message)"
        $script:ErrorShown = $true
        $script:ExitCode = 1
    }
    Write-Host ''
    if ($Uninstall) {
        Write-Host 'FAILED: Dice Wars was NOT fully removed.'
    } elseif ($script:InstalledAt) {
        Write-Host "FAILED: Dice Wars was copied to $($script:InstalledAt) but did not pass its checks."
    } else {
        Write-Host 'FAILED: Dice Wars was NOT installed.'
    }
    if ($script:Step -gt 0) {
        Write-Host ("  It stopped at step {0} of {1}: {2}." -f $script:Step, $script:TotalSteps, $script:StepTitle)
    }
    Write-Host '  Nothing that was already set up is undone, so after fixing the problem above'
    Write-Host '  you can run the same command again and it continues from there:'
    Write-Host "    $script:RerunCommand"
    Write-Host ("  Time spent: {0}." -f (Get-Elapsed))
    exit $script:ExitCode
} finally {
    if ($script:WorkDir -and (Test-Path -LiteralPath $script:WorkDir)) {
        Remove-Item -LiteralPath $script:WorkDir -Recurse -Force -ErrorAction SilentlyContinue
    }
}
