param(
    [Parameter(Mandatory=$true)][long]$Window,
    [Parameter(Mandatory=$true)][int]$ProcessId,
    [Parameter(Mandatory=$true)][string]$FirstFile,
    [Parameter(Mandatory=$true)][int]$ExpectedCount,
    [Parameter(Mandatory=$true)][string]$LastFile
)
$ErrorActionPreference='Stop'
[Console]::OutputEncoding=[System.Text.UTF8Encoding]::new($false)
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
$rootElement=[System.Windows.Automation.AutomationElement]::FromHandle([IntPtr]$Window)
if($rootElement.Current.ProcessId -ne $ProcessId) {throw 'Photos window ownership changed'}
function Find-Control([string]$Id) {
    $condition=[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,$Id)
    return $rootElement.FindFirst([System.Windows.Automation.TreeScope]::Descendants,$condition)
}
$toggle=Find-Control 'ToggleFilmstripButton'
if($null -eq $toggle) {throw 'Photos filmstrip toggle is missing'}
$filmstrip=Find-Control 'MultiSelectGridView'
if($null -eq $filmstrip -or $filmstrip.Current.IsOffscreen) {
    ([System.Windows.Automation.InvokePattern]$toggle.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern)).Invoke()
    $deadline=[DateTime]::UtcNow.AddSeconds(5)
    do {
        Start-Sleep -Milliseconds 30
        $filmstrip=Find-Control 'MultiSelectGridView'
    } while(($null -eq $filmstrip -or $filmstrip.Current.IsOffscreen) -and [DateTime]::UtcNow -lt $deadline)
}
if($null -eq $filmstrip -or $filmstrip.Current.IsOffscreen) {throw 'Photos filmstrip did not become visible'}
$preparationStart=[DateTime]::UtcNow
$deadline=[DateTime]::UtcNow.AddSeconds(12)
$last=$null
if($ExpectedCount -le 40) {
    do {
        $last=$filmstrip.FindFirst([System.Windows.Automation.TreeScope]::Children,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::NameProperty,$LastFile))
        if($null -ne $last) {break}
        Start-Sleep -Milliseconds 100
    } while([DateTime]::UtcNow -lt $deadline)
    if($null -eq $last) {throw 'Photos has not enumerated the final small-folder entry'}
}
$deadline=[DateTime]::UtcNow.AddSeconds(12)
do {
    $items=$filmstrip.FindAll([System.Windows.Automation.TreeScope]::Children,[System.Windows.Automation.Condition]::TrueCondition)
    $itemCount=@($items | Where-Object {$_.Current.ControlType -eq [System.Windows.Automation.ControlType]::ListItem}).Count
    if($itemCount -ge [Math]::Min($ExpectedCount,40)) {break}
    Start-Sleep -Milliseconds 100
} while([DateTime]::UtcNow -lt $deadline)
$names=@()
$first=$null
foreach($item in $items) {
    if($item.Current.ControlType -eq [System.Windows.Automation.ControlType]::ListItem) {
        $names+=$item.Current.Name
        if($item.Current.Name -eq $FirstFile) {$first=$item}
    }
}
if($null -eq $first) {throw 'Initial fixture is missing from Photos filmstrip'}
$first.SetFocus()
if($rootElement.Current.ProcessId -ne $ProcessId) {throw 'Photos window ownership changed'}
$image=Find-Control 'D2DImageControl'
[pscustomobject]@{
    files=$names
    focused_file=$FirstFile
    image_rect=$image.Current.BoundingRectangle.ToString()
    filmstrip_rect=$filmstrip.Current.BoundingRectangle.ToString()
    last_entry_available=($null -ne $last)
    boundary_navigation_eligible=($ExpectedCount -le 40)
    enumeration_preparation_ms=([DateTime]::UtcNow-$preparationStart).TotalMilliseconds
} | ConvertTo-Json -Depth 4 -Compress
