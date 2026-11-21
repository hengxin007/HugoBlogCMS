param([string]$Proxy)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$release = 'https://github.com/gohugoio/hugo/releases/download/v0.130.0'
$archiveName = 'hugo_extended_0.130.0_windows-amd64.zip'
$checksumsName = 'hugo_0.130.0_checksums.txt'
$download = New-Item -ItemType Directory -Path ([IO.Path]::Combine([IO.Path]::GetTempPath(), 'hugo-cms-' + [guid]::NewGuid()))
$binaryDirectory = Join-Path $PSScriptRoot '..\src-tauri\binaries'
$resourcesDirectory = Join-Path $PSScriptRoot '..\src-tauri\licenses'
try {
    $network = @{ TimeoutSec = 120 }
    if ($Proxy) { $network.Proxy = $Proxy }
    foreach ($name in @($archiveName, $checksumsName)) {
        Invoke-WebRequest @network -Uri "$release/$name" -OutFile (Join-Path $download.FullName $name)
    }
    $archive = Join-Path $download.FullName $archiveName
    $checksumLine = Get-Content -LiteralPath (Join-Path $download.FullName $checksumsName) |
        Where-Object { $_ -match ('\s' + [regex]::Escape($archiveName) + '$') } |
        Select-Object -First 1
    if (!$checksumLine -or (Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash -ne ($checksumLine -split '\s+')[0]) {
        throw 'Hugo SHA-256 verification failed.'
    }
    $expanded = Join-Path $download.FullName 'expanded'
    Expand-Archive -LiteralPath $archive -DestinationPath $expanded
    New-Item -ItemType Directory -Force -Path $binaryDirectory, $resourcesDirectory | Out-Null
    Copy-Item -LiteralPath (Join-Path $expanded 'hugo.exe') -Destination (Join-Path $binaryDirectory 'hugo-x86_64-pc-windows-msvc.exe')
    foreach ($document in (Get-ChildItem -LiteralPath $expanded -File | Where-Object Name -match '^(LICENSE|NOTICE)')) {
        $text = [IO.File]::ReadAllText($document.FullName) -replace '\r?\n', "`r`n"
        [IO.File]::WriteAllText((Join-Path $resourcesDirectory ('Hugo-' + $document.Name)), $text, [Text.UTF8Encoding]::new($false))
    }
} finally {
    $cleanup = $download.FullName
    $tempRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\') + '\'
    if ($cleanup.StartsWith($tempRoot, [StringComparison]::OrdinalIgnoreCase) -and $download.Name -match '^hugo-cms-[a-f0-9-]{36}$') {
        Remove-Item -LiteralPath $cleanup -Recurse -Force
    }
}
