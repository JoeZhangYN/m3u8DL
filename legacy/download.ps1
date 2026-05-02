# 交互模式：双击 download.bat 进入；让用户粘贴 m3u8 / URL / 文件路径
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
. (Join-Path $PSScriptRoot 'pipeline.ps1')

Write-Host ''
Write-Host '=== M3U8 下载器（PNG 反爬版 / 交互模式）===' -ForegroundColor Cyan
Write-Host ''
Write-Host '可输入（三选一）：' -ForegroundColor DarkGray
Write-Host '  - m3u8 文本（#EXTM3U 开头，DevTools Response 全选 Ctrl+C 后直接粘贴）'
Write-Host '  - m3u8 URL（http/https 开头）'
Write-Host '  - 本地 m3u8 文件路径'
Write-Host ''
$src = Read-Host '请粘贴'
if (-not $src) { Write-Host '未输入' -ForegroundColor Red; exit 1 }
$src = $src.Trim().Trim('"').Trim("'")

$name = Read-Host '保存文件名（不含 .mp4，留空自动生成）'

$result = Invoke-M3u8Download -Source $src -Name $name -ScriptDir $PSScriptRoot
if (-not $result.Success) { exit 1 }

Write-Host ''
$keep = Read-Host '保留临时文件？(y/N)'
if ($keep -ne 'y' -and $keep -ne 'Y') {
    Remove-Item -LiteralPath $result.WorkDir -Recurse -Force -ErrorAction SilentlyContinue
}
