#Requires -Version 5.1
<#
.SYNOPSIS
  Windows / PowerShell host smoke for patchloom (create, replace, doc, tx, paths).

.DESCRIPTION
  Fixrealloop-style dogfood under PowerShell: Windows path separators, absolute
  paths, JSON error_kind peels, insert-after, contain, multi-op tx.
  Not part of make check (Linux/macOS gate). Run on Windows or via CI ci-windows.

.PARAMETER Bin
  Path to patchloom.exe (default: target\debug\patchloom.exe relative to repo root).

.EXAMPLE
  pwsh -File scripts/windows-smoke.ps1 -Bin target\debug\patchloom.exe
#>
param(
    [string]$Bin = ""
)

$ErrorActionPreference = "Stop"
$script:Passes = 0
$script:Fails = 0

function Pass([string]$Msg) {
    Write-Host "OK: $Msg"
    $script:Passes++
}
function Fail([string]$Msg) {
    Write-Host "FAIL: $Msg" -ForegroundColor Red
    $script:Fails++
}

# Resolve binary
$RepoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
if (-not $Bin) {
    $candidates = @(
        (Join-Path $RepoRoot "target\debug\patchloom.exe"),
        (Join-Path $RepoRoot "target\release\patchloom.exe"),
        (Join-Path $RepoRoot "target\debug\patchloom"),
        (Join-Path $RepoRoot "target\release\patchloom")
    )
    foreach ($c in $candidates) {
        if (Test-Path -LiteralPath $c) { $Bin = $c; break }
    }
}
if (-not $Bin -or -not (Test-Path -LiteralPath $Bin)) {
    throw "patchloom binary not found. Build first (cargo build --all-features) or pass -Bin"
}
$Bin = (Resolve-Path -LiteralPath $Bin).Path
Write-Host "BIN=$Bin"

function Invoke-Pl {
    param(
        [Parameter(ValueFromRemainingArguments = $true)]
        [string[]]$Args
    )
    $out = & $Bin @Args 2>&1 | Out-String
    return @{
        ExitCode = $LASTEXITCODE
        Output   = $out
    }
}

function Get-JsonField {
    param([string]$JsonText, [string]$Name)
    try {
        $obj = $JsonText | ConvertFrom-Json
        $prop = $obj.PSObject.Properties[$Name]
        if ($null -eq $prop) { return $null }
        return $prop.Value
    } catch {
        return $null
    }
}

$ws = Join-Path ([System.IO.Path]::GetTempPath()) ("pl-win-smoke-" + [guid]::NewGuid().ToString("N").Substring(0, 8))
New-Item -ItemType Directory -Path $ws | Out-Null
try {
    Push-Location $ws
    git init -q 2>$null | Out-Null
    git commit --allow-empty -q -m init 2>$null | Out-Null

    # --- create / already_exists / force ---
    Set-Content -Path (Join-Path $ws "taken.txt") -Value "old`n" -NoNewline
    $r = Invoke-Pl --json --cwd $ws create taken.txt --content "new"
    $kind = Get-JsonField $r.Output "error_kind"
    $applied = Get-JsonField $r.Output "applied"
    if ($r.ExitCode -eq 1 -and $kind -eq "already_exists" -and ($applied -eq $false -or "$applied" -eq "False")) {
        Pass "create dest-exists already_exists (PowerShell)"
    } else {
        $snip = if ($r.Output.Length -gt 200) { $r.Output.Substring(0, 200) } else { $r.Output }
        Fail "create dest-exists exit=$($r.ExitCode) kind=$kind applied=$applied out=$snip"
    }
    if ($r.Output -match "force") {
        Pass "already_exists message mentions force"
    } else {
        Fail "already_exists message missing force hint: $($r.Output)"
    }
    $r = Invoke-Pl --json --cwd $ws create taken.txt --content "forced`n" --force --apply
    $got = Get-Content -LiteralPath (Join-Path $ws "taken.txt") -Raw
    if ($r.ExitCode -eq 0 -and $got -match "forced") {
        Pass "create --force --apply"
    } else {
        Fail "create force exit=$($r.ExitCode) content=$got"
    }

    # --- replace with backslash-style path under --cwd ---
    $sub = Join-Path $ws "sub\nested"
    New-Item -ItemType Directory -Path $sub -Force | Out-Null
    Set-Content -LiteralPath (Join-Path $sub "app.rs") -Value "use std::io;`nfn main() {}`n"
    # Relative path with backslashes (Windows agent style)
    $rel = "sub\nested\app.rs"
    $r = Invoke-Pl --cwd $ws replace "use std::io;" --insert-after "use std::fs;" $rel --apply
    $body = Get-Content -LiteralPath (Join-Path $sub "app.rs") -Raw
    if ($r.ExitCode -eq 0 -and $body -match "use std::io;" -and $body -match "use std::fs;") {
        Pass "insert-after with backslash relative path"
    } else {
        Fail "insert-after exit=$($r.ExitCode) body=$body out=$($r.Output)"
    }

    # --- absolute Windows path ---
    $absFile = Join-Path $ws "abs_target.txt"
    Set-Content -LiteralPath $absFile -Value "hello world`n"
    $r = Invoke-Pl replace "world" --new "windows" $absFile --apply
    $body = Get-Content -LiteralPath $absFile -Raw
    if ($r.ExitCode -eq 0 -and $body -match "hello windows") {
        Pass "replace via absolute path"
    } else {
        Fail "abs replace exit=$($r.ExitCode) body=$body"
    }

    # --- doc set on JSON ---
    $cfg = Join-Path $ws "config.json"
    Set-Content -LiteralPath $cfg -Value '{"server":{"port":8080}}' -NoNewline
    $r = Invoke-Pl --cwd $ws doc set config.json server.port 9090 --apply
    $cfgBody = Get-Content -LiteralPath $cfg -Raw
    if ($r.ExitCode -eq 0 -and $cfgBody -match "9090") {
        Pass "doc set"
    } else {
        Fail "doc set exit=$($r.ExitCode) body=$cfgBody"
    }

    # --- tx plan with key alias (ops + key) ---
    $plan = Join-Path $ws "plan.json"
    @'
{"ops":[{"op":"doc.set","path":"config.json","key":"server.port","value":7070}]}
'@ | Set-Content -LiteralPath $plan -NoNewline
    $r = Invoke-Pl --cwd $ws tx plan.json --apply
    $cfgBody = Get-Content -LiteralPath $cfg -Raw
    if ($r.ExitCode -eq 0 -and $cfgBody -match "7070") {
        Pass "tx plan key alias"
    } else {
        Fail "tx exit=$($r.ExitCode) body=$cfgBody out=$($r.Output)"
    }

    # --- multi-op tx create two files ---
    $plan2 = Join-Path $ws "plan2.json"
    @{
        version    = 1
        operations = @(
            @{ op = "file.create"; path = "t1.txt"; content = "a`n" }
            @{ op = "file.create"; path = "t2.txt"; content = "b`n" }
        )
    } | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath $plan2
    $r = Invoke-Pl --cwd $ws --json tx plan2.json --apply
    if ($r.ExitCode -eq 0 -and (Test-Path (Join-Path $ws "t1.txt")) -and (Test-Path (Join-Path $ws "t2.txt"))) {
        Pass "tx multi file.create"
    } else {
        Fail "tx multi exit=$($r.ExitCode) out=$($r.Output)"
    }

    # --- preview exit 2, no write ---
    Set-Content -LiteralPath (Join-Path $ws "prev.txt") -Value "alpha`n"
    $r = Invoke-Pl --cwd $ws replace alpha --new beta prev.txt
    $still = Get-Content -LiteralPath (Join-Path $ws "prev.txt") -Raw
    if ($r.ExitCode -eq 2 -and $still -match "alpha") {
        Pass "preview exit 2 no write"
    } else {
        Fail "preview exit=$($r.ExitCode) content=$still"
    }

    # --- --contain rejects escape (Windows absolute outside workspace) ---
    Set-Content -LiteralPath (Join-Path $ws "in.txt") -Value "x`n"
    # Use a path that is clearly outside the temp workspace
    $escape = "C:\Windows\System32\drivers\etc\hosts"
    $r = Invoke-Pl --json --cwd $ws --contain read $escape
    $kind = Get-JsonField $r.Output "error_kind"
    if ($kind -eq "guard_rejected") {
        Pass "contain escape guard_rejected"
    } else {
        $snip = if ($r.Output.Length -gt 250) { $r.Output.Substring(0, 250) } else { $r.Output }
        Fail "contain kind=$kind exit=$($r.ExitCode) out=$snip"
    }

    # --- not_found delete ---
    $r = Invoke-Pl --json --cwd $ws delete missing-nope.txt --apply
    $kind = Get-JsonField $r.Output "error_kind"
    if ($kind -eq "not_found" -and $r.ExitCode -eq 1) {
        Pass "delete missing not_found"
    } else {
        Fail "delete kind=$kind exit=$($r.ExitCode)"
    }

    # --- rename dest already_exists ---
    Set-Content -LiteralPath (Join-Path $ws "from.txt") -Value "a`n"
    Set-Content -LiteralPath (Join-Path $ws "to.txt") -Value "b`n"
    $r = Invoke-Pl --json --cwd $ws rename from.txt to.txt --apply
    $kind = Get-JsonField $r.Output "error_kind"
    if ($kind -eq "already_exists") {
        Pass "rename dest already_exists"
    } else {
        Fail "rename kind=$kind out=$($r.Output)"
    }

    # --- batch PATH OLD NEW ---
    Set-Content -LiteralPath (Join-Path $ws "ver.txt") -Value "v1`n"
    $ops = Join-Path $ws "ops.txt"
    "replace ver.txt v1 v2" | Set-Content -LiteralPath $ops
    $r = Invoke-Pl --cwd $ws batch ops.txt --apply
    $v = Get-Content -LiteralPath (Join-Path $ws "ver.txt") -Raw
    if ($r.ExitCode -eq 0 -and $v -match "v2") {
        Pass "batch replace PATH OLD NEW"
    } else {
        Fail "batch exit=$($r.ExitCode) ver=$v"
    }

    # --- search no_matches exit 3 ---
    Set-Content -LiteralPath (Join-Path $ws "only.txt") -Value "only`n"
    $r = Invoke-Pl --json --cwd $ws search nomatch only.txt
    if ($r.ExitCode -eq 3) {
        Pass "search no_matches exit 3"
    } else {
        Fail "search exit=$($r.ExitCode)"
    }

    # --- CRLF file insert-after keeps CR LF ---
    $crlf = Join-Path $ws "crlf.txt"
    [System.IO.File]::WriteAllBytes($crlf, [byte[]](0x6c, 0x31, 0x0d, 0x0a, 0x6c, 0x32, 0x0d, 0x0a)) # l1\r\nl2\r\n
    $r = Invoke-Pl --cwd $ws replace "l1" --insert-after "// post" crlf.txt --apply
    $bytes = [System.IO.File]::ReadAllBytes($crlf)
    $hex = ($bytes | ForEach-Object { $_.ToString("x2") }) -join " "
    # Expect l1\r\n// post\r\n...
    $asText = [System.Text.Encoding]::UTF8.GetString($bytes)
    if ($r.ExitCode -eq 0 -and $asText -match "l1\r\n// post\r\n") {
        Pass "CRLF insert-after preserves line endings"
    } elseif ($r.ExitCode -eq 0 -and $asText -match "// post") {
        # Still applied; mixed endings would be a product bug
        if ($asText -match "l1\n// post" -and $asText -notmatch "l1\r\n// post") {
            Fail "CRLF insert used bare LF: $hex"
        } else {
            Pass "CRLF insert-after applied ($hex)"
        }
    } else {
        Fail "CRLF exit=$($r.ExitCode) hex=$hex"
    }

    # --- version ---
    $r = Invoke-Pl --version
    if ($r.ExitCode -eq 0 -and $r.Output -match "patchloom") {
        Pass "version"
    } else {
        Fail "version: $($r.Output)"
    }

} finally {
    Pop-Location
    Remove-Item -LiteralPath $ws -Recurse -Force -ErrorAction SilentlyContinue
}

Write-Host "==== windows-smoke: passes=$script:Passes fails=$script:Fails ===="
if ($script:Fails -gt 0) {
    exit 1
}
exit 0
