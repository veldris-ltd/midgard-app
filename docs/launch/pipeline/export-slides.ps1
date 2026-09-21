param([string]$Deck, [string]$OutDir)
$app = New-Object -ComObject PowerPoint.Application
$pres = $app.Presentations.Open($Deck, $true, $false, $false)
if (Test-Path $OutDir) { Remove-Item -Recurse -Force $OutDir }
New-Item -ItemType Directory -Force $OutDir | Out-Null
$pres.Export($OutDir, "PNG", 1920, 1080)
$pres.Close()
$app.Quit()
Get-ChildItem $OutDir | Select-Object -ExpandProperty Name
