# ============================================================
# STARFORGE LAN server (no dependencies, PowerShell 5.1+)
#  - HTTP static file server : port 17888  (serves the game)
#  - WebSocket relay         : port 17889  (multiplayer messages)
# ============================================================
$ErrorActionPreference = 'Continue'
$root = Split-Path -Parent $MyInvocation.MyCommand.Path
$httpPort = 17888
$wsPort = 17889

$mime = @{
  '.html' = 'text/html; charset=utf-8'
  '.js'   = 'text/javascript; charset=utf-8'
  '.css'  = 'text/css; charset=utf-8'
  '.png'  = 'image/png'
  '.jpg'  = 'image/jpeg'
  '.ico'  = 'image/x-icon'
  '.json' = 'application/json'
}

Write-Host '============================================='
Write-Host ' STARFORGE LAN SERVER RUNNING'
Write-Host '============================================='
Write-Host (' Host plays at : http://localhost:{0}' -f $httpPort)
try {
  $ips = [System.Net.Dns]::GetHostAddresses([System.Net.Dns]::GetHostName()) |
    Where-Object { $_.AddressFamily -eq 'InterNetwork' -and $_.ToString() -notlike '169.254.*' -and $_.ToString() -ne '127.0.0.1' }
  foreach ($ip in $ips) { Write-Host (' Friends join  : http://{0}:{1}' -f $ip.ToString(), $httpPort) }
} catch {}
Write-Host ' (allow access if Windows Firewall asks)'
Write-Host ' Press Ctrl+C to stop.'
Write-Host ''

$httpListener = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Any, $httpPort)
$wsListener = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Any, $wsPort)
$httpListener.Start()
$wsListener.Start()

$sha1 = [System.Security.Cryptography.SHA1]::Create()
$clients = New-Object System.Collections.ArrayList
$nextId = 1

function Read-HttpRequest($stream){
  $buf = New-Object System.IO.MemoryStream
  $b = New-Object byte[] 4096
  $deadline = [DateTime]::Now.AddSeconds(5)
  while ([DateTime]::Now -lt $deadline){
    $n = $stream.Read($b, 0, $b.Length)
    if ($n -le 0) { break }
    $buf.Write($b, 0, $n)
    $txt = [Text.Encoding]::ASCII.GetString($buf.ToArray())
    if ($txt.Contains("`r`n`r`n")) { return $txt }
  }
  return [Text.Encoding]::ASCII.GetString($buf.ToArray())
}

function Send-HttpFile($client, $reqText){
  $stream = $client.GetStream()
  try {
    $line = ($reqText -split "`r`n")[0]
    $parts = $line -split ' '
    $path = if ($parts.Length -ge 2) { $parts[1] } else { '/' }
    $path = $path.Split('?')[0]
    if ($path -eq '/') { $path = '/index.html' }
    $path = [Uri]::UnescapeDataString($path) -replace '/', '\'
    $full = Join-Path $root $path.TrimStart('\')
    $fullResolved = [System.IO.Path]::GetFullPath($full)
    # 路径边界校验：必须等于根目录或以「根目录+分隔符」开头（防 wrsk_backup 之类前缀绕过）
    $rootBound = $root + [System.IO.Path]::DirectorySeparatorChar
    $insideRoot = ($fullResolved -eq $root) -or $fullResolved.StartsWith($rootBound, [StringComparison]::OrdinalIgnoreCase)
    if (-not $insideRoot -or -not (Test-Path -LiteralPath $fullResolved -PathType Leaf)){
      $body = [Text.Encoding]::UTF8.GetBytes('404 Not Found')
      $hdr = "HTTP/1.1 404 Not Found`r`nContent-Length: $($body.Length)`r`nConnection: close`r`n`r`n"
      $hb = [Text.Encoding]::ASCII.GetBytes($hdr)
      $stream.Write($hb, 0, $hb.Length); $stream.Write($body, 0, $body.Length)
    } else {
      $ext = [System.IO.Path]::GetExtension($fullResolved).ToLower()
      $ct = if ($mime.ContainsKey($ext)) { $mime[$ext] } else { 'application/octet-stream' }
      # 静态缓存：ETag（修改时间+大小）支持 304；HTML 保持 no-cache（版本更新可靠传播），其余资源 24h 缓存
      $fi = Get-Item -LiteralPath $fullResolved
      $etag = [BitConverter]::ToString([Text.Encoding]::ASCII.GetBytes($fi.LastWriteTimeUtc.Ticks.ToString() + '-' + $fi.Length)).Replace('-','')
      $cacheCtl = if ($ext -eq '.html') { 'no-cache' } else { 'max-age=86400' }
      $inm = $null
      if ($reqText -match 'If-None-Match:\s*"?([A-Za-z0-9]+)"?') { $inm = $Matches[1] }
      if ($inm -and $inm -eq $etag){
        $hdr = "HTTP/1.1 304 Not Modified`r`nETag: `"$etag`"`r`nCache-Control: $cacheCtl`r`nConnection: close`r`n`r`n"
        $hb = [Text.Encoding]::ASCII.GetBytes($hdr)
        $stream.Write($hb, 0, $hb.Length)
      } else {
        $bytes = [System.IO.File]::ReadAllBytes($fullResolved)
        $hdr = "HTTP/1.1 200 OK`r`nContent-Type: $ct`r`nContent-Length: $($bytes.Length)`r`nCache-Control: $cacheCtl`r`nETag: `"$etag`"`r`nConnection: close`r`n`r`n"
        $hb = [Text.Encoding]::ASCII.GetBytes($hdr)
        $stream.Write($hb, 0, $hb.Length)
        $stream.Write($bytes, 0, $bytes.Length)
      }
    }
  } catch {}
  try { $client.Close() } catch {}
}

function New-WsFrame([string]$text){
  $payload = [Text.Encoding]::UTF8.GetBytes($text)
  $len = $payload.Length
  $ms = New-Object System.IO.MemoryStream
  $ms.WriteByte(0x81)
  if ($len -lt 126){ $ms.WriteByte([byte]$len) }
  elseif ($len -lt 65536){
    $ms.WriteByte(126)
    $ms.WriteByte([byte](($len -shr 8) -band 0xFF))
    $ms.WriteByte([byte]($len -band 0xFF))
  } else {
    $ms.WriteByte(127)
    $lenBytes = [BitConverter]::GetBytes([UInt64]$len)
    [Array]::Reverse($lenBytes)   # big-endian
    $ms.Write($lenBytes, 0, 8)
  }
  $ms.Write($payload, 0, $len)
  return $ms.ToArray()
}

function Send-Ws($cl, [string]$text){
  try {
    $frame = New-WsFrame $text
    $cl.Stream.Write($frame, 0, $frame.Length)
  } catch { $cl.Dead = $true }
}

function Broadcast([string]$text, $except){
  foreach ($cl in @($clients)){
    if ($cl -ne $except -and -not $cl.Dead){ Send-Ws $cl $text }
  }
}

function Accept-WsClient($tcp){
  $tcp.SendTimeout = 5000   # 写阻塞超时：僵尸连接写满缓冲不再卡死主循环
  $stream = $tcp.GetStream()
  $req = Read-HttpRequest $stream
  if ($req -match 'Sec-WebSocket-Key:\s*(\S+)'){
    $key = $Matches[1]
    $accept = [Convert]::ToBase64String($sha1.ComputeHash([Text.Encoding]::ASCII.GetBytes($key + '258EAFA5-E914-47DA-95CA-C5AB0DC85B11')))
    $hdr = "HTTP/1.1 101 Switching Protocols`r`nUpgrade: websocket`r`nConnection: Upgrade`r`nSec-WebSocket-Accept: $accept`r`n`r`n"
    $hb = [Text.Encoding]::ASCII.GetBytes($hdr)
    $stream.Write($hb, 0, $hb.Length)
    $cl = [PSCustomObject]@{ Tcp = $tcp; Stream = $stream; Id = $script:nextId; Buf = (New-Object System.IO.MemoryStream); Frag = (New-Object System.IO.MemoryStream); FragOp = 0; Dead = $false }
    $script:nextId++
    [void]$clients.Add($cl)
    Send-Ws $cl ('{"t":"ws-id","id":' + $cl.Id + '}')
    Write-Host ("[+] player {0} connected ({1})" -f $cl.Id, $tcp.Client.RemoteEndPoint)
  } else {
    try { $tcp.Close() } catch {}
  }
}

function Pump-WsClient($cl){
  $stream = $cl.Stream
  try {
    while ($stream.DataAvailable){
      $b = New-Object byte[] 8192
      $n = $stream.Read($b, 0, $b.Length)
      if ($n -le 0){ $cl.Dead = $true; return }
      $cl.Buf.Write($b, 0, $n)
    }
  } catch { $cl.Dead = $true; return }
  # parse complete frames from buffer
  while ($true){
    $data = $cl.Buf.ToArray()
    if ($data.Length -lt 2) { return }
    $opcode = $data[0] -band 0x0F
    $masked = ($data[1] -band 0x80) -ne 0
    $len = $data[1] -band 0x7F
    $off = 2
    if ($len -eq 126){
      if ($data.Length -lt 4) { return }
      # 必须显式转 int：$data[x] 是 Byte，Byte -shl 8 会回绕（1 -shl 8 = 0），导致 ≥256 字节的帧长度解析错误
      $len = ([int]$data[2] -shl 8) -bor [int]$data[3]; $off = 4
    } elseif ($len -eq 127){
      if ($data.Length -lt 10) { return }
      $len = [long]0
      for ($i = 2; $i -lt 10; $i++){ $len = ($len * 256) + $data[$i] }
      $off = 10
    }
    # 帧长上限 16MB（含未完成分片累计）：防恶意/异常客户端声明巨帧撑爆宿主内存
    if ($len -gt 16777216 -or ($cl.Frag.Length + $len) -gt 16777216){ $cl.Dead = $true; return }
    $maskLen = if ($masked) { 4 } else { 0 }
    if ($data.Length -lt $off + $maskLen + $len) { return }
    $payload = New-Object byte[] $len
    if ($masked){
      $mask = $data[$off..($off + 3)]
      for ($i = 0; $i -lt $len; $i++){ $payload[$i] = $data[$off + 4 + $i] -bxor $mask[$i % 4] }
    } else {
      [Array]::Copy($data, $off, $payload, 0, $len)
    }
    # keep remainder
    $consumed = $off + $maskLen + $len
    $cl.Buf.SetLength(0)
    if ($consumed -lt $data.Length){ $cl.Buf.Write($data, $consumed, $data.Length - $consumed) }
    switch ($opcode){
      0 { # 续帧：追加，FIN 到达后组装完整消息并中继
        if ($cl.Frag.Length -gt 0 -or $len -gt 0){
          $cl.Frag.Write($payload, 0, $len)
          if (($data[0] -band 0x80) -ne 0){
            Broadcast ([Text.Encoding]::UTF8.GetString($cl.Frag.ToArray())) $cl
            $cl.Frag.SetLength(0)
          }
        }
      }
      1 { if ($cl.Frag.Length -gt 0){ $cl.Frag.SetLength(0) }    # 新消息头帧 → 丢弃未完成分片
        if (($data[0] -band 0x80) -ne 0){ Broadcast ([Text.Encoding]::UTF8.GetString($payload)) $cl }   # 完整单帧
        else { $cl.Frag.Write($payload, 0, $len) }   # 分片开始
      }
      8 { try {
          $closeFrame = New-Object byte[] 2
          $closeFrame[0] = 0x88; $closeFrame[1] = 0
          $cl.Stream.Write($closeFrame, 0, 2)
        } catch {}; $cl.Dead = $true; return }
      9 { try { $pong = New-Object byte[] 2; $pong[0] = 0x8A; $pong[1] = 0; $cl.Stream.Write($pong, 0, 2) } catch { $cl.Dead = $true } }
    }
  }
}

# ---------------- main loop ----------------
while ($true){
  $work = $false
  # http
  while ($httpListener.Pending()){
    $work = $true
    $tcp = $httpListener.AcceptTcpClient()
    $tcp.SendTimeout = 5000   # 慢速客户端也不阻塞文件下发
    try {
      $req = Read-HttpRequest $tcp.GetStream()
      Send-HttpFile $tcp $req
    } catch { try { $tcp.Close() } catch {} }
  }
  # ws accept
  while ($wsListener.Pending()){
    $work = $true
    try { Accept-WsClient ($wsListener.AcceptTcpClient()) } catch {}
  }
  # ws pump
  foreach ($cl in @($clients)){
    if (-not $cl.Dead){
      try {
        # 对端已断开（收到 FIN 且无残留数据）：立即下线，防止僵尸连接累积、写满缓冲后阻塞中继
        if ($cl.Tcp.Client.Poll(0, [System.Net.Sockets.SelectMode]::SelectRead) -and $cl.Tcp.Available -eq 0){ $cl.Dead = $true; continue }
        if ($cl.Stream.DataAvailable){ $work = $true }
        Pump-WsClient $cl
      } catch { $cl.Dead = $true }
    }
  }
  # drop dead
  foreach ($cl in @($clients | Where-Object { $_.Dead })){
    [void]$clients.Remove($cl)
    try { $cl.Tcp.Close() } catch {}
    Broadcast ('{"t":"left","id":' + $cl.Id + '}') $null
    Write-Host ("[-] player {0} left" -f $cl.Id)
  }
  # 有流量时也保证每轮至少 1ms 休眠：空转会烧满一个核（房主自己也要流畅游玩）
  if ($work){ Start-Sleep -Milliseconds 1 } else { Start-Sleep -Milliseconds 8 }
}
