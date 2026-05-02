# 共享 pipeline：PNG 剥皮 / m3u8 normalize / RE 下载 / ffmpeg 重封装
# 被 download.ps1 (交互) 和 download_server.ps1 (HTTP server) dot-source 复用

# Site-agnostic baseline (no Origin/Referer here — those come from the client per request,
# auto-derived from the playing page by capture.user.js).
#
# SOT NOTE: this hashtable is a copy of m3u8dl-rs/src/config.rs::DEFAULT_HEADERS.
# When the Rust UA / Accept headers change, sync this block too (legacy fallback only —
# divergence is low-impact but still worth keeping aligned for predictable behavior).
$script:DefaultHeaders = @{
    'User-Agent'      = 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/147.0.0.0 Safari/537.36 Edg/147.0.0.0'
    'Accept'          = '*/*'
    'Accept-Language' = 'zh-CN,zh;q=0.9,en;q=0.8'
}

function Merge-Headers {
    # Build the final headers map: defaults <- client-supplied <- (page-derived fallback)
    param(
        [hashtable]$ClientHeaders,
        [string]$PageUrl
    )
    $h = @{}
    foreach ($k in $script:DefaultHeaders.Keys) { $h[$k] = $script:DefaultHeaders[$k] }
    if ($ClientHeaders) {
        foreach ($k in $ClientHeaders.Keys) { $h[$k] = [string]$ClientHeaders[$k] }
    }
    if (-not ($h.ContainsKey('Origin')) -and $PageUrl) {
        try {
            $u = [System.Uri]$PageUrl
            $h['Origin']  = "$($u.Scheme)://$($u.Authority)"
            $h['Referer'] = $PageUrl
        } catch { }
    }
    return $h
}

function Get-SystemProxyUrl {
    # 读 WinINET 系统代理（Edge / Chrome / IE 共用）；返 http://host:port 或 $null
    try {
        $ie = Get-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Internet Settings' -ErrorAction Stop
        if ($ie.ProxyEnable -ne 1 -or -not $ie.ProxyServer) { return $null }
        $server = [string]$ie.ProxyServer
        # 形如 "http=127.0.0.1:7890;https=127.0.0.1:7890" 时优先取 https，再退 http
        if ($server -match 'https=([^;]+)')   { $server = $Matches[1] }
        elseif ($server -match 'http=([^;]+)') { $server = $Matches[1] }
        if ($server -match '^socks') { return $null }  # RE 不支持 socks 直传
        if ($server -notmatch '^https?://') { $server = "http://$server" }
        return $server
    } catch {
        return $null
    }
}

function Strip-PngWrapper {
    param([byte[]]$Bytes)
    if ($Bytes.Length -lt 16) { return $Bytes }
    if ($Bytes[0] -ne 0x89 -or $Bytes[1] -ne 0x50 -or $Bytes[2] -ne 0x4E -or $Bytes[3] -ne 0x47) { return $Bytes }
    $i = 8; $end = $Bytes.Length - 8
    while ($i -le $end) {
        $i = [Array]::IndexOf($Bytes, [byte]0x49, $i)
        if ($i -lt 0 -or $i -gt $end) { break }
        if ($Bytes[$i+1] -eq 0x45 -and $Bytes[$i+2] -eq 0x4E -and $Bytes[$i+3] -eq 0x44 -and
            $Bytes[$i+4] -eq 0xAE -and $Bytes[$i+5] -eq 0x42 -and $Bytes[$i+6] -eq 0x60 -and $Bytes[$i+7] -eq 0x82) {
            $start = $i + 8
            $len = $Bytes.Length - $start
            $out = New-Object byte[] $len
            [Array]::Copy($Bytes, $start, $out, 0, $len)
            return $out
        }
        $i++
    }
    return $Bytes
}

function ConvertTo-NormalizedM3u8 {
    param([string]$Text)
    # DevTools 复制时整段压一行 → 在每个 #EXT / http(s):// 前补换行
    if ($Text -match '#EXTM3U' -and ($Text -split "[\r\n]+").Count -lt 5) {
        $Text = [regex]::Replace($Text, '\s+(?=#EXT)', "`n")
        $Text = [regex]::Replace($Text, '\s+(?=https?://)', "`n")
        $Text = $Text.Trim()
    }
    # 缺 ENDLIST 的话补一个，否则 RE 当直播流崩
    if ($Text -notmatch '#EXT-X-ENDLIST') {
        $Text = $Text.TrimEnd() + "`n#EXT-X-ENDLIST`n"
    }
    return $Text
}

function Resolve-InputType {
    param([string]$Src)
    if ($Src -match '^#EXTM3U') { return 'raw' }
    if ($Src -match '^https?://') { return 'url' }
    if (Test-Path -LiteralPath $Src -PathType Leaf) { return 'file' }
    return 'unknown'
}

function Invoke-M3u8Download {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$Source,
        [string]$Name,
        [string]$OutDir = 'C:\Folder\Download',
        [hashtable]$Headers,
        [hashtable]$ClientHeaders,  # from POST body — Origin/Referer/Cookie auto-derived by capture.user.js
        [string]$PageUrl,           # fallback source for Origin/Referer if ClientHeaders empty
        [int]$Threads = 16,
        [string]$ScriptDir = $PSScriptRoot,
        [string]$LogFile,
        [scriptblock]$Log,
        [string]$SourceUrl  # raw 模式下也提供原 m3u8 URL → 用作 --base-url 解析相对分片路径
    )

    if (-not $Headers) { $Headers = Merge-Headers -ClientHeaders $ClientHeaders -PageUrl $PageUrl }
    if (-not $Name) { $Name = 'video_' + (Get-Date -Format 'yyyyMMdd_HHmmss') }
    $Name = $Name -replace '[\\/:*?"<>|]', '_'
    if (-not (Test-Path $OutDir)) { New-Item -ItemType Directory -Path $OutDir -Force | Out-Null }

    $WorkDir = Join-Path $env:TEMP ('m3u8dl_' + [guid]::NewGuid().ToString('N').Substring(0, 8))
    New-Item -ItemType Directory -Path $WorkDir -Force | Out-Null

    $write = {
        param($msg, $color = 'Gray')
        if ($LogFile) { Add-Content -LiteralPath $LogFile -Value $msg -Encoding UTF8 }
        if ($Log) { & $Log $msg $color }
        else { Write-Host $msg -ForegroundColor $color }
    }

    & $write "[m3u8dl] work=$WorkDir name=$Name" 'DarkGray'

    try {
        $type = Resolve-InputType -Src $Source
        if ($type -eq 'unknown') { throw "输入既非 #EXTM3U 文本、也非 URL、也非现有文件（前 80 字符: $($Source.Substring(0, [Math]::Min(80, $Source.Length)))）" }

        $rawFile   = Join-Path $WorkDir 'playlist.raw'
        $cleanFile = Join-Path $WorkDir 'playlist.m3u8'
        $sourceUrl = if ($SourceUrl) { [System.Net.WebUtility]::HtmlDecode($SourceUrl) } else { $null }

        switch ($type) {
            'raw'  { [System.IO.File]::WriteAllText($rawFile, $Source, [System.Text.UTF8Encoding]::new($false)) }
            'file' { Copy-Item -LiteralPath $Source $rawFile -Force }
            'url'  {
                $sourceUrl = [System.Net.WebUtility]::HtmlDecode($Source)
                Invoke-WebRequest -Uri $sourceUrl -Headers $Headers -OutFile $rawFile -UseBasicParsing -ErrorAction Stop
            }
        }
        & $write "[1/3] 输入解析完毕（$type）" 'Cyan'

        $clean = Strip-PngWrapper -Bytes ([System.IO.File]::ReadAllBytes($rawFile))
        $text  = [System.Text.Encoding]::UTF8.GetString($clean)
        $text  = ConvertTo-NormalizedM3u8 -Text $text
        if ($text -notmatch '#EXTM3U') { throw "响应剥皮后仍非 m3u8（前 200 字符: " + ($text.Substring(0, [Math]::Min(200, $text.Length)) -replace '[^\x20-\x7e]', '.') + ")" }
        [System.IO.File]::WriteAllText($cleanFile, $text, [System.Text.UTF8Encoding]::new($false))

        $segCount = ([regex]::Matches($text, '(?m)^[^#\s].+$')).Count
        $totalDur = ([regex]::Matches($text, '#EXTINF:([\d.]+)') | ForEach-Object { [double]$_.Groups[1].Value } | Measure-Object -Sum).Sum
        & $write "  分片 $segCount 个 / 时长 $([Math]::Round($totalDur/60,1)) 分钟" 'Green'
        $firstSeg = ([regex]::Match($text, '(?m)^[^#\s].+$')).Value
        if ($firstSeg) { & $write "  首片: $firstSeg" 'DarkGray' }

        $reArgs = @(
            $cleanFile,
            '--save-dir', $WorkDir,
            '--save-name', 'merged',
            '--tmp-dir', $WorkDir,
            '--ffmpeg-binary-path', (Join-Path (Split-Path $ScriptDir -Parent) 'ffmpeg.exe'),
            '--binary-merge', '--auto-select', '--del-after-done',
            '--thread-count', $Threads, '--download-retry-count', '3',
            '--http-request-timeout', '30',
            '--no-date-info', '--no-log', '--disable-update-check',
            '--check-segments-count', 'False'
        )
        if ($sourceUrl) {
            $u = [System.Uri]$sourceUrl
            # 取 m3u8 所在目录（去掉文件名 + query），RE 用此前缀解析相对分片路径
            $basePath = $u.AbsolutePath -replace '/[^/]*$', '/'
            $baseUrl = "$($u.Scheme)://$($u.Authority)$basePath"
            $reArgs += @('--base-url', $baseUrl)
            & $write "  base-url: $baseUrl" 'DarkGray'
        }
        foreach ($k in $Headers.Keys) { $reArgs += '-H'; $reArgs += "${k}: $($Headers[$k])" }

        $proxyUrl = Get-SystemProxyUrl
        if ($proxyUrl) {
            $reArgs += @('--custom-proxy', $proxyUrl)
            & $write "  代理: $proxyUrl" 'DarkGray'
        } else {
            & $write "  代理: 无（系统代理未启用）" 'DarkGray'
        }

        & $write "[2/3] N_m3u8DL-RE 下载 ..." 'Cyan'
        $reExe = Join-Path $ScriptDir 'N_m3u8DL-RE-v0.5.1-beta.exe'
        $reOutput = & $reExe @reArgs 2>&1 | ForEach-Object { $_.ToString() }
        if ($LogFile) { Add-Content -LiteralPath $LogFile -Value ($reOutput -join "`n") -Encoding UTF8 }
        if ($LASTEXITCODE -ne 0) {
            $tail = if ($reOutput) { ($reOutput | Select-Object -Last 30) -join "`n" } else { '(no output)' }
            throw "N_m3u8DL-RE exit=$LASTEXITCODE`n--- RE tail ---`n$tail"
        }

        $merged = Get-ChildItem $WorkDir -Filter 'merged*' -File |
                  Where-Object { $_.Extension -in '.ts', '.mp4', '.mkv' } |
                  Sort-Object Length -Descending | Select-Object -First 1
        if (-not $merged) { throw "找不到 RE 合并产物" }

        & $write "[3/3] ffmpeg -f mpegts 重封装 ..." 'Cyan'
        $outFile = Join-Path $OutDir "$Name.mp4"
        & (Join-Path (Split-Path $ScriptDir -Parent) 'ffmpeg.exe') -y -hide_banner -loglevel warning `
            -f mpegts -i $merged.FullName -c copy -bsf:a aac_adtstoasc $outFile
        if (-not (Test-Path $outFile) -or (Get-Item $outFile).Length -lt 1024) { throw "ffmpeg 输出无效" }

        $sizeMB = [Math]::Round((Get-Item $outFile).Length / 1MB, 2)
        & $write "完成: $outFile ($sizeMB MB)" 'Green'

        return [PSCustomObject]@{ Success = $true; OutputPath = $outFile; SizeMB = $sizeMB; WorkDir = $WorkDir; Error = $null }
    }
    catch {
        & $write "失败: $($_.Exception.Message)" 'Red'
        return [PSCustomObject]@{ Success = $false; OutputPath = $null; SizeMB = 0; WorkDir = $WorkDir; Error = $_.Exception.Message }
    }
    finally {
        # 默认保留 WorkDir 让调用方决定是否清理
    }
}
