# Windows PowerShell 5.1 and PowerShell 7. Host execution policy is respected.
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0
. (Join-Path $PSScriptRoot 'downloads.ps1')

function Save-TopazBoundedStream {
    param($InputStream, [string]$Destination, [long]$ExpectedBytes, [switch]$FromExtractor)
    $buffer = New-Object byte[] 65536
    $copied = 0L
    $file = [IO.File]::Open($Destination, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
    try {
        while ($true) {
            if ($FromExtractor) {
                $pending = $InputStream.ReadAsync($buffer, 0, $buffer.Length)
                if (-not $pending.Wait(30000)) { throw 'The archive extractor stopped making progress.' }
                $count = $pending.GetAwaiter().GetResult()
            } else {
                $count = $InputStream.Read($buffer, 0, $buffer.Length)
            }
            if ($count -eq 0) { break }
            if ($count -gt ($ExpectedBytes - $copied)) { throw 'The received file exceeds its pinned byte count.' }
            $file.Write($buffer, 0, $count)
            $copied += $count
        }
        if ($copied -ne $ExpectedBytes) { throw 'The received file is shorter than its pinned byte count.' }
    } finally {
        $file.Dispose()
    }
}

function Save-TopazDownload {
    param([string]$Uri, [string]$Destination, [long]$ExpectedBytes)
    $current = [Uri]$Uri
    $previousTls = [Net.ServicePointManager]::SecurityProtocol
    try {
        # Enable the HTTPS protocol needed by GitHub without changing a stored
        # machine/user setting or weakening certificate verification.
        [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
        for ($redirect = 0; $redirect -le 10; $redirect++) {
            if ($current.Scheme -cne 'https') { throw 'Core Kit downloads require HTTPS at every redirect.' }
            $request = [Net.HttpWebRequest]::Create($current)
            $request.AllowAutoRedirect = $false
            $request.UseDefaultCredentials = $false
            $request.UserAgent = 'Topaz-Core-Kit-Installer/1'
            $request.Timeout = 30000
            $request.ReadWriteTimeout = 30000
            $response = $null
            try {
                $response = $request.GetResponse()
                $status = [int]$response.StatusCode
                if ($status -ge 300 -and $status -le 399) {
                    $location = $response.Headers['Location']
                    if ([string]::IsNullOrEmpty($location)) { throw 'The release redirect has no destination.' }
                    $current = New-Object Uri($current, $location)
                    continue
                }
                if ($status -ne 200) { throw "The release request returned HTTP $status." }
                if ($response.ContentLength -ge 0 -and $response.ContentLength -ne $ExpectedBytes) {
                    throw 'The release response byte count differs from the catalogue.'
                }
                $downloadStream = $response.GetResponseStream()
                try {
                    Save-TopazBoundedStream $downloadStream $Destination $ExpectedBytes
                } finally {
                    $downloadStream.Dispose()
                }
                return
            } finally {
                if ($null -ne $response) { $response.Dispose() }
                $request.Abort()
            }
        }
        throw 'The release request exceeded its redirect limit.'
    } finally {
        [Net.ServicePointManager]::SecurityProtocol = $previousTls
    }
}

function Read-TopazArchiveExecutable {
    param([string]$Tar, [string]$Archive, [string]$Destination, [long]$ExpectedBytes, [string]$Directory)
    $info = New-Object Diagnostics.ProcessStartInfo
    $info.FileName = $Tar
    $info.Arguments = ConvertTo-TopazWindowsArguments @('-xOzf', $Archive, 'topaz-core-kit.exe')
    $info.WorkingDirectory = $Directory
    $info.UseShellExecute = $false
    $info.CreateNoWindow = $true
    $info.RedirectStandardInput = $true
    $info.RedirectStandardOutput = $true
    $info.RedirectStandardError = $true
    $process = New-Object Diagnostics.Process
    $process.StartInfo = $info
    $started = $false
    try {
        $started = $process.Start()
        if (-not $started) { throw 'Could not start the archive extractor.' }
        $process.StandardInput.Close()
        $errorRead = $process.StandardError.ReadToEndAsync()
        # PowerShell text redirection must not touch native executable bytes.
        Save-TopazBoundedStream $process.StandardOutput.BaseStream $Destination $ExpectedBytes -FromExtractor
        if (-not $process.WaitForExit(30000)) { throw 'The archive extractor did not finish.' }
        $errorText = $errorRead.GetAwaiter().GetResult()
        if ($process.ExitCode -ne 0) {
            if ($errorText.Length -gt 2048) { $errorText = $errorText.Substring(0, 2048) }
            throw ("The archive extractor failed. " + $errorText)
        }
    } finally {
        if ($started -and -not $process.HasExited) { $process.Kill(); $process.WaitForExit() }
        $process.Dispose()
    }
}

$temporary = $null
$archive = $null
$candidate = $null
$code = 1
try {
    $sourceArchive = $null
    if ($args.Count -eq 1 -and ($args[0] -ceq '--help' -or $args[0] -ceq '-h')) {
        Write-Output 'Install the Core Kit release selected by this checkout.'
        Write-Output 'Usage'
        Write-Output '  install-core-kit.cmd [--archive FILE]'
        Write-Output 'Requires tar.exe and Windows PowerShell 5.1 or newer with local script execution allowed by host policy.'
        Write-Output 'The optional archive is still checked against the pinned catalogue.'
        Write-Output 'For a local source build without PowerShell scripts, use bootstrap.cmd.'
        $code = 0
    } else {
        if ($args.Count -ne 0) {
            if ($args.Count -ne 2 -or $args[0] -cne '--archive' -or [string]::IsNullOrEmpty($args[1])) {
                $code = 2
                throw 'Use install-core-kit.cmd [--archive FILE].'
            }
            $sourceArchive = (Get-TopazRegularFile ([string]$args[1])).FullName
        }
        $root = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')).Path
        $target = Get-TopazHostTarget
        $download = Get-TopazDownload $root $target
        $tar = (Get-Command tar.exe -CommandType Application -ErrorAction Stop).Source
        $temporary = Join-Path ([IO.Path]::GetTempPath()) ('topaz-core-kit-' + [Guid]::NewGuid().ToString('N'))
        $null = New-Item -ItemType Directory -Path $temporary -ErrorAction Stop
        $archive = Join-Path $temporary 'asset.tar.gz'
        $candidate = Join-Path $temporary 'topaz-core-kit.exe'
        if ($null -eq $sourceArchive) {
            $uri = 'https://github.com/' + $download.Repository + '/releases/download/' + $download.Tag + '/' + $download.AssetName
            Write-Output ("Downloading " + $download.Tag + ' for ' + $target)
            Save-TopazDownload $uri $archive $download.AssetBytes
        } else {
            $archiveStream = [IO.File]::Open($sourceArchive, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read)
            try {
                Save-TopazBoundedStream $archiveStream $archive $download.AssetBytes
            } finally {
                $archiveStream.Dispose()
            }
        }
        Assert-TopazFileIdentity $archive $download.AssetBytes $download.AssetSha256
        Read-TopazArchiveExecutable $tar $archive $candidate $download.ExecutableBytes $temporary
        Assert-TopazFileIdentity $candidate $download.ExecutableBytes $download.ExecutableSha256
        $info = New-Object Diagnostics.ProcessStartInfo
        $info.FileName = $candidate
        $info.Arguments = ConvertTo-TopazWindowsArguments @('kit', 'install', '--archive', $archive)
        $info.UseShellExecute = $false
        # First run has only the pinned executable. Keep the caller's directory
        # out of the Windows loader's current-directory search.
        $info.WorkingDirectory = $temporary
        $info.EnvironmentVariables['TOPAZ_CORE_KIT_ROOT'] = $root
        $process = [Diagnostics.Process]::Start($info)
        try { $process.WaitForExit(); $code = $process.ExitCode }
        finally { $process.Dispose() }
    }
} catch {
    [Console]::Error.WriteLine($_.Exception.Message)
    [Console]::Error.WriteLine('Existing local installations were preserved. Source builds remain available through bootstrap.cmd.')
} finally {
    foreach ($path in @($archive, $candidate)) {
        if ($null -ne $path -and [IO.File]::Exists($path)) {
            try { [IO.File]::Delete($path) } catch { [Console]::Error.WriteLine('Could not remove a private installer temporary file.') }
        }
    }
    if ($null -ne $temporary -and [IO.Directory]::Exists($temporary)) {
        try { [IO.Directory]::Delete($temporary, $false) } catch { [Console]::Error.WriteLine("Installer temporary directory retained at $temporary") }
    }
}
exit $code
