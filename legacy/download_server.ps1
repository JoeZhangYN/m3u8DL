# HTTP server：监听 127.0.0.1:7787，接 Tampermonkey POST 的 m3u8 文本，后台跑 download
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8

$ScriptDir = $PSScriptRoot
. (Join-Path $ScriptDir 'pipeline.ps1')

$Port = 7787
$LogDir = Join-Path $ScriptDir 'server_logs'
if (-not (Test-Path $LogDir)) { New-Item -ItemType Directory -Path $LogDir -Force | Out-Null }

# job 状态表（单线程 listener，无并发，普通 hashtable 够）
$jobs = @{}

function Write-Json {
    param($Response, [int]$Status, $Body)
    $Response.StatusCode = $Status
    $Response.ContentType = 'application/json; charset=utf-8'
    $bytes = [System.Text.Encoding]::UTF8.GetBytes(($Body | ConvertTo-Json -Depth 6 -Compress))
    $Response.ContentLength64 = $bytes.Length
    $Response.OutputStream.Write($bytes, 0, $bytes.Length)
}

function Add-CorsHeaders {
    param($Response)
    $Response.Headers.Add('Access-Control-Allow-Origin', '*')
    $Response.Headers.Add('Access-Control-Allow-Methods', 'GET, POST, OPTIONS')
    $Response.Headers.Add('Access-Control-Allow-Headers', 'Content-Type')
}

function Get-JobSnapshot {
    param($id)
    $j = $jobs[$id]
    if (-not $j) { return $null }
    $state = if ($j.Job) { $j.Job.State.ToString() } else { 'unknown' }
    if ($state -eq 'Completed' -and -not $j.Result) {
        # 收割结果
        $j.Result = Receive-Job -Job $j.Job -Keep -ErrorAction SilentlyContinue
        Remove-Job -Job $j.Job -Force -ErrorAction SilentlyContinue
        $j.Job = $null
    }
    return @{
        id        = $id
        title     = $j.Title
        state     = $state
        startedAt = $j.StartedAt.ToString('s')
        success   = $j.Result.Success
        output    = $j.Result.OutputPath
        sizeMB    = $j.Result.SizeMB
        error     = $j.Result.Error
    }
}

$listener = [System.Net.HttpListener]::new()
$listener.Prefixes.Add("http://127.0.0.1:$Port/")
try {
    $listener.Start()
} catch {
    Write-Host "无法绑定 127.0.0.1:$Port — $($_.Exception.Message)" -ForegroundColor Red
    Write-Host '可能端口被占用或需要管理员权限。' -ForegroundColor Yellow
    exit 1
}

Write-Host ''
Write-Host "=== M3U8 下载 Server ===" -ForegroundColor Cyan
Write-Host "监听: http://127.0.0.1:$Port" -ForegroundColor Green
Write-Host "脚本: $ScriptDir"
Write-Host "日志: $LogDir"
Write-Host "停止: Ctrl+C"
Write-Host ''

while ($listener.IsListening) {
    try {
        $ctx = $listener.GetContext()
    } catch {
        break
    }
    $req = $ctx.Request
    $res = $ctx.Response
    Add-CorsHeaders -Response $res
    $path = $req.Url.AbsolutePath
    $ts = Get-Date -Format 'HH:mm:ss'
    Write-Host "[$ts] $($req.HttpMethod) $path" -ForegroundColor DarkGray

    try {
        if ($req.HttpMethod -eq 'OPTIONS') {
            $res.StatusCode = 204
        }
        elseif ($req.HttpMethod -eq 'GET' -and $path -eq '/ping') {
            Write-Json -Response $res -Status 200 -Body @{ ok = $true; port = $Port; jobs = $jobs.Count }
        }
        elseif ($req.HttpMethod -eq 'GET' -and $path -eq '/status') {
            $list = $jobs.Keys | ForEach-Object { Get-JobSnapshot -id $_ }
            Write-Json -Response $res -Status 200 -Body @{ jobs = @($list) }
        }
        elseif ($req.HttpMethod -eq 'POST' -and $path -eq '/download') {
            $body = (New-Object IO.StreamReader($req.InputStream, [Text.Encoding]::UTF8)).ReadToEnd()
            $data = $body | ConvertFrom-Json
            if (-not $data.m3u8) { Write-Json -Response $res -Status 400 -Body @{ error = 'm3u8 字段缺失' }; continue }

            $id = [guid]::NewGuid().ToString('N').Substring(0, 8)
            $title = if ($data.title) { $data.title } else { "video_$id" }
            $logFile = Join-Path $LogDir "$id.log"

            $sourceUrl = if ($data.url) { [string]$data.url } else { '' }
            $page = if ($data.page) { [string]$data.page } else { '' }
            # Convert PSCustomObject -> hashtable so Invoke-M3u8Download merges correctly
            $clientHeaders = @{}
            if ($data.headers) {
                foreach ($p in $data.headers.PSObject.Properties) { $clientHeaders[$p.Name] = [string]$p.Value }
            }
            $job = Start-ThreadJob -Name "m3u8dl_$id" -InitializationScript ([scriptblock]::Create(". '$ScriptDir\pipeline.ps1'")) -ScriptBlock {
                param($m3u8, $title, $scriptDir, $logFile, $srcUrl, $clientHdrs, $pageUrl)
                Invoke-M3u8Download -Source $m3u8 -Name $title -ScriptDir $scriptDir -LogFile $logFile -SourceUrl $srcUrl -ClientHeaders $clientHdrs -PageUrl $pageUrl
            } -ArgumentList $data.m3u8, $title, $ScriptDir, $logFile, $sourceUrl, $clientHeaders, $page

            $jobs[$id] = @{ Job = $job; Title = $title; StartedAt = Get-Date; Page = $page; SourceUrl = $sourceUrl; Result = $null }
            Write-Host "  → 入队 [$id] $title" -ForegroundColor Green
            Write-Json -Response $res -Status 202 -Body @{ jobId = $id; status = 'queued'; title = $title }
        }
        elseif ($req.HttpMethod -eq 'GET' -and $path -match '^/job/([0-9a-f]+)$') {
            $id = $Matches[1]
            $snap = Get-JobSnapshot -id $id
            if (-not $snap) { Write-Json -Response $res -Status 404 -Body @{ error = "job $id not found" } }
            else { Write-Json -Response $res -Status 200 -Body $snap }
        }
        else {
            Write-Json -Response $res -Status 404 -Body @{ error = "unknown route: $path" }
        }
    } catch {
        Write-Host "  ! 处理出错: $($_.Exception.Message)" -ForegroundColor Red
        try { Write-Json -Response $res -Status 500 -Body @{ error = $_.Exception.Message } } catch {}
    } finally {
        try { $res.Close() } catch {}
    }
}
