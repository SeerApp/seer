# Install seer.exe + libz3.dll from a GitHub release tarball (or -Archive).
param(
    [string]$Prefix = "",
    [string]$Archive = ""
)

$ErrorActionPreference = "Stop"
$Repo = "SeerApp/seer"

function Get-Arch {
    switch ($env:PROCESSOR_ARCHITECTURE) {
        "AMD64" { return "amd64" }
        "ARM64" { return "arm64" }
        default { throw "Unsupported PROCESSOR_ARCHITECTURE: $($env:PROCESSOR_ARCHITECTURE)" }
    }
}

function Get-InstallDir {
    if ($Prefix -ne "") {
        New-Item -ItemType Directory -Force -Path $Prefix | Out-Null
        return $Prefix
    }
    $dir = Join-Path $env:LOCALAPPDATA "seer"
    New-Item -ItemType Directory -Force -Path $dir | Out-Null
    return $dir
}

function Install-FromArchive([string]$path) {
    $tmp = Join-Path ([System.IO.Path]::GetTempPath()) ("seer-install-" + [guid]::NewGuid().ToString("n"))
    New-Item -ItemType Directory -Force -Path $tmp | Out-Null
    try {
        tar -xzf $path -C $tmp
        $exe = Join-Path $tmp "seer.exe"
        $dll = Join-Path $tmp "libz3.dll"
        if (-not (Test-Path $exe)) { throw "seer.exe not found in archive." }
        if (-not (Test-Path $dll)) { throw "libz3.dll not found in archive." }
        $dest = Get-InstallDir
        Copy-Item -Force $exe (Join-Path $dest "seer.exe")
        Copy-Item -Force $dll (Join-Path $dest "libz3.dll")
        Write-Host "Installed to $(Join-Path $dest 'seer.exe')"
        $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
        if ($userPath -notlike "*$dest*") {
            [Environment]::SetEnvironmentVariable("Path", "$dest;$userPath", "User")
            Write-Host "[INFO] Added $dest to your user PATH. Open a new terminal."
        }
    }
    finally {
        Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
    }
}

$arch = Get-Arch
$filename = "seer-windows-$arch.tar.gz"

if ($Archive -ne "") {
    if (-not (Test-Path $Archive)) { throw "archive not found: $Archive" }
    $sumFile = "$Archive.sha256"
    if (Test-Path $sumFile) {
        $expect = ((Get-Content $sumFile -Raw) -split '\s+')[0]
        $actual = (Get-FileHash -Algorithm SHA256 $Archive).Hash.ToLower()
        if ($actual -ne $expect.ToLower()) { throw "checksum mismatch for $Archive" }
    }
    Install-FromArchive $Archive
    return
}

$rel = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/latest"
$tag = $rel.tag_name
$url = "https://github.com/$Repo/releases/download/$tag/$filename"
$tmpZip = Join-Path ([System.IO.Path]::GetTempPath()) $filename
Write-Host "Downloading $url"
Invoke-WebRequest -Uri $url -OutFile $tmpZip
try {
    Install-FromArchive $tmpZip
}
finally {
    Remove-Item -Force $tmpZip -ErrorAction SilentlyContinue
}
