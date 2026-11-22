$ErrorActionPreference = 'Stop'
$d = '<games-555>\<v555-4>'
$exe = Join-Path $d '<main-exe>'
$p = Start-Process -FilePath $exe -WorkingDirectory $d -PassThru
Start-Sleep -Seconds 5
$p.Refresh()
[pscustomobject]@{
  Id = $p.Id
  HasExited = $p.HasExited
  ExitCode = if ($p.HasExited) { $p.ExitCode } else { $null }
  Title = $p.MainWindowTitle
  Responding = $p.Responding
} | Format-List
