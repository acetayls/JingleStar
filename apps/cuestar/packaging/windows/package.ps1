# Construit l'installeur et la version portable de CueStar pour Windows.
# À lancer depuis la racine du dépôt MediaStar, après `cargo build --release -p cuestar` :
#   powershell -ExecutionPolicy Bypass -File apps\cuestar\packaging\windows\package.ps1
# libmpv-2.dll est pris dans mpv\ à la racine du dépôt (build « mpv-dev » de shinchiro),
# ou à l'endroit donné par -Mpv ; MediaInfo.dll (MediaInfo « DLL » 64 bits de mediaarea.net)
# dans le même dossier, ou à l'endroit donné par -MediaInfo. Inno Setup 6 doit être installé (ISCC.exe).
param(
    [string]$Mpv = "mpv\libmpv-2.dll",
    [string]$MediaInfo = "mpv\MediaInfo.dll",
    [string]$Out = "dist"
)
$ErrorActionPreference = "Stop"

$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$app = Resolve-Path (Join-Path $here "..\..")
$version = (Select-String -Path (Join-Path $app "Cargo.toml") -Pattern '^version\s*=\s*"(.+)"' | Select-Object -First 1).Matches[0].Groups[1].Value
$exe = "target\release\cuestar.exe"
if (-not (Test-Path $exe)) { throw "Introuvable : $exe (lancer cargo build --release -p cuestar)" }
if (-not (Test-Path $Mpv)) { throw "Introuvable : $Mpv (copier libmpv-2.dll du build mpv-dev de shinchiro)" }
if (-not (Test-Path $MediaInfo)) { throw "Introuvable : $MediaInfo (MediaInfo DLL Windows x64 sur mediaarea.net)" }

New-Item -ItemType Directory -Force -Path $Out | Out-Null
$Out = (Resolve-Path $Out).Path
$stage = Join-Path $Out "stage-windows"
if (Test-Path $stage) { Remove-Item -Recurse -Force $stage }
New-Item -ItemType Directory -Force -Path (Join-Path $stage "licences") | Out-Null
Copy-Item $exe $stage
Copy-Item $Mpv (Join-Path $stage "libmpv-2.dll")
Copy-Item $MediaInfo (Join-Path $stage "MediaInfo.dll")
# Nom du build shinchiro utilisé (écrit par le workflow), cité dans les licences.
$build = Join-Path (Split-Path -Parent $Mpv) "mpv-build.txt"
if (Test-Path $build) { Copy-Item $build (Join-Path $stage "licences\mpv-build.txt") }
$miVersion = Join-Path (Split-Path -Parent $MediaInfo) "mediainfo-version.txt"
if (Test-Path $miVersion) { Copy-Item $miVersion (Join-Path $stage "licences\mediainfo-version.txt") }
Copy-Item (Join-Path $here "..\licences\*") (Join-Path $stage "licences") -Recurse

# Version portable : un zip à décompresser n'importe où.
$zip = Join-Path $Out "CueStar-$version-windows-x64-portable.zip"
if (Test-Path $zip) { Remove-Item -Force $zip }
Compress-Archive -Path (Join-Path $stage "*") -DestinationPath $zip
Write-Host "Créé : $zip"

# Installeur
$iscc = (Get-Command ISCC.exe -ErrorAction SilentlyContinue).Source
if (-not $iscc) {
    foreach ($p in @("${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe", "$env:ProgramFiles\Inno Setup 6\ISCC.exe", "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe")) {
        if (Test-Path $p) { $iscc = $p; break }
    }
}
if (-not $iscc) { throw "Inno Setup 6 introuvable (winget install JRSoftware.InnoSetup)" }
& $iscc "/DVersion=$version" "/DStage=$stage" "/DOutDir=$Out" (Join-Path $here "cuestar.iss")
if ($LASTEXITCODE -ne 0) { throw "ISCC a échoué ($LASTEXITCODE)" }
Write-Host "Créé : $Out\CueStar-$version-windows-x64-setup.exe"
