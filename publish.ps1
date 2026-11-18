param(
    [string]$Version = ""
)

$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot

$cargoPath = Join-Path $PSScriptRoot "Cargo.toml"
$cargoContent = Get-Content $cargoPath -Raw

if ([string]::IsNullOrWhiteSpace($Version)) {
    if ($cargoContent -match 'version\s*=\s*"(\d+)\.(\d+)\.(\d+)"') {
        $maj = [int]$Matches[1]
        $min = [int]$Matches[2]
        $pat = [int]$Matches[3]
        Write-Host "Phien ban hien tai trong Cargo.toml: v$maj.$min.$pat" -ForegroundColor Cyan
        $inputVer = Read-Host "Nhap phien ban moi (Enter de giu nguyen v$maj.$min.$pat, hoac go 'bump' de tang len v$maj.$min.$($pat+1))"
        if ($inputVer -eq "bump") {
            $Version = "$maj.$min.$($pat+1)"
        } elseif (-not [string]::IsNullOrWhiteSpace($inputVer)) {
            $Version = $inputVer.TrimStart('v', 'V')
        } else {
            $Version = "$maj.$min.$pat"
        }
    }
} else {
    $Version = $Version.TrimStart('v', 'V')
}

# Update Cargo.toml version if changed
$newCargo = [regex]::Replace($cargoContent, '(?m)^version\s*=\s*"[^"]+"', "version = `"$Version`"")
if ($newCargo -ne $cargoContent) {
    Set-Content -Path $cargoPath -Value $newCargo -Encoding UTF8 -NoNewline
    Write-Host "Da cap nhat Cargo.toml -> v$Version" -ForegroundColor Green
}

Write-Host "1. Dang bien dich ban Release v$Version..." -ForegroundColor Yellow
Stop-Process -Name "hi75-battery","Leobog-Hi75C-Pro-Battery" -Force -ErrorAction SilentlyContinue
cargo build --release --bin hi75-battery
if ($LASTEXITCODE -ne 0) {
    throw "Loi bien dich cargo build!"
}

# Prepare dist folder
$distDir = Join-Path $PSScriptRoot "dist"
New-Item -ItemType Directory -Force -Path $distDir | Out-Null
$exePath = Join-Path $distDir "Leobog-Hi75C-Pro-Battery.exe"
Copy-Item "D:\hi75-battery\target\release\hi75-battery.exe" $exePath -Force

# Update local installed copy & launch
$localInstallDir = Join-Path $env:LOCALAPPDATA "LeobogHi75Battery"
if (Test-Path $localInstallDir) {
    Copy-Item $exePath (Join-Path $localInstallDir "Leobog-Hi75C-Pro-Battery.exe") -Force -ErrorAction SilentlyContinue
}
explorer.exe "D:\hi75-battery\target\release\hi75-battery.exe"

Write-Host "2. Dang commit va day code len GitHub..." -ForegroundColor Yellow
$remotes = git remote
if ($remotes -notcontains "origin") {
    git remote add origin "https://github.com/tranlong-mechatronics/hi75-battery.git"
}
git add -A
$status = git status --porcelain
if ($status) {
    git commit -m "Release v$Version (Kuromi Edition)"
}
git tag -f "v$Version"
git push -u origin master --force
git push origin "v$Version" --force

Write-Host "3. Dang phat hanh bản cap nhat v$Version len GitHub Releases..." -ForegroundColor Yellow
$cred = "protocol=https`nhost=github.com`n" | git credential fill
$token = ($cred | Where-Object { $_ -match "^password=" }) -replace "^password=", ""
$headers = @{
    Authorization = "Bearer $token"
    "User-Agent"  = "hi75-battery-publisher"
    Accept        = "application/vnd.github+json"
}

$tag = "v$Version"
$repoApi = "https://api.github.com/repos/tranlong-mechatronics/hi75-battery"

# Check if release with this tag already exists; if so, delete it so we can recreate cleanly
try {
    $existing = Invoke-RestMethod -Uri "$repoApi/releases/tags/$tag" -Headers $headers
    if ($existing.id) {
        Invoke-RestMethod -Method Delete -Uri "$repoApi/releases/$($existing.id)" -Headers $headers | Out-Null
    }
} catch {}

$releaseBody = @{
    tag_name   = $tag
    name       = "Leobog Hi75C Pro Battery Monitor $tag (Kuromi Edition)"
    body       = "Bản cài đặt 1-Click theo dõi pin bàn phím Leobog Hi75C Pro kèm Kuromi trên thanh Taskbar (Hỗ trợ Tự động Cập nhật)."
    draft      = $false
    prerelease = $false
} | ConvertTo-Json -Depth 4

$release = Invoke-RestMethod -Method Post -Uri "$repoApi/releases" -Headers $headers -Body ([System.Text.Encoding]::UTF8.GetBytes($releaseBody)) -ContentType "application/json; charset=utf-8"

$uploadUrl = ($release.upload_url -replace '\{\?name,label\}', '') + "?name=Leobog-Hi75C-Pro-Battery.exe"
$uploadHeaders = @{
    Authorization = "Bearer $token"
    "User-Agent"  = "hi75-battery-publisher"
    "Content-Type" = "application/vnd.microsoft.portable-executable"
}
$asset = Invoke-RestMethod -Method Post -Uri $uploadUrl -Headers $uploadHeaders -InFile $exePath

Write-Host ""
Write-Host "==========================================================" -ForegroundColor Green
Write-Host " PHAT HANH THANH CONG BAN CAP NHAT $tag!" -ForegroundColor Green
Write-Host "==========================================================" -ForegroundColor Green
Write-Host "Link tai 1-Click cho nguoi yeu (luon la ban moi nhat):" -ForegroundColor Cyan
Write-Host "https://github.com/tranlong-mechatronics/hi75-battery/releases/latest/download/Leobog-Hi75C-Pro-Battery.exe" -ForegroundColor White
Write-Host "Trang GitHub Release:" -ForegroundColor Cyan
Write-Host "$($release.html_url)" -ForegroundColor White
