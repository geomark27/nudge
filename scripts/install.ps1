# ================================================================
#  Nudge - Instalador para Windows
#  Uso:      iwr -useb https://raw.githubusercontent.com/geomark27/nudge/main/scripts/install.ps1 | iex
#  Version:  $env:NUDGE_VERSION = "v0.1.0"; iwr -useb ... | iex
#  Sirve tambien para actualizar: reemplaza la version instalada y conserva tus datos.
# ================================================================

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"   # sin barra de progreso, Invoke-WebRequest va mucho mas rapido
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

$repo       = "geomark27/nudge"
$installDir = "$env:LOCALAPPDATA\Nudge"

# Reintenta ante cortes de red; un 404 significa que esa version no esta publicada.
function Get-Release($url) {
    for ($try = 1; $try -le 3; $try++) {
        try {
            return Invoke-RestMethod $url -TimeoutSec 30
        } catch {
            $code = $_.Exception.Response.StatusCode.value__
            if ($code -eq 404) {
                throw "No hay una version publicada ($(if ($env:NUDGE_VERSION) { $env:NUDGE_VERSION } else { 'latest' })). Revisa https://github.com/$repo/releases"
            }
            if ($try -eq 3) { throw "No se pudo conectar con GitHub: $($_.Exception.Message)" }
            Write-Host "      sin respuesta de GitHub, reintentando ($try/3)..." -ForegroundColor DarkYellow
            Start-Sleep -Seconds (2 * $try)
        }
    }
}

Write-Host ""
Write-Host "  Nudge - Instalador" -ForegroundColor Cyan
Write-Host "  ------------------------------------" -ForegroundColor Cyan
Write-Host ""

# 1. Buscar la version (la ultima, o la indicada en NUDGE_VERSION)
Write-Host "[1/4] Buscando version..." -ForegroundColor Yellow
if ($env:NUDGE_VERSION) {
    $api = "https://api.github.com/repos/$repo/releases/tags/$($env:NUDGE_VERSION)"
} else {
    $api = "https://api.github.com/repos/$repo/releases/latest"
}
$release = Get-Release $api
$version = $release.tag_name
# Nudge-windows-x64-setup.exe (hasta v0.1.0: Nudge_<version>_x64-setup.exe)
$asset   = $release.assets | Where-Object { $_.name -like "*x64-setup.exe" } | Select-Object -First 1
if (-not $asset) { throw "El release $version no tiene instalador para Windows." }
Write-Host "      version: $version" -ForegroundColor Green

# 2. Descargar el instalador
Write-Host "[2/4] Descargando $($asset.name)..." -ForegroundColor Yellow
$installer = Join-Path $env:TEMP $asset.name
Invoke-WebRequest -Uri $asset.browser_download_url -OutFile $installer -TimeoutSec 300
Write-Host "      descargado en $installer" -ForegroundColor Green

# 3. Instalar en silencio (por usuario, sin permisos de administrador)
Write-Host "[3/4] Instalando..." -ForegroundColor Yellow
Get-Process -Name nudge -ErrorAction SilentlyContinue | Stop-Process -Force
$proc = Start-Process -FilePath $installer -ArgumentList "/S" -Wait -PassThru
Remove-Item $installer -Force -ErrorAction SilentlyContinue
if ($proc.ExitCode -ne 0) { throw "El instalador termino con codigo $($proc.ExitCode)." }
Write-Host "      instalado en $installDir" -ForegroundColor Green

# 4. Abrir Nudge (queda en la bandeja del sistema)
Write-Host "[4/4] Iniciando Nudge..." -ForegroundColor Yellow
$exe = Get-ChildItem $installDir -Filter *.exe -ErrorAction SilentlyContinue |
    Where-Object { $_.Name -notlike "uninstall*" } | Select-Object -First 1
if ($exe) {
    Start-Process $exe.FullName
    Write-Host "      listo" -ForegroundColor Green
} else {
    Write-Host "      abrelo desde el menu Inicio" -ForegroundColor Green
}

Write-Host ""
Write-Host "  Nudge $version instalado correctamente" -ForegroundColor Green
Write-Host ""
Write-Host "  Proximos pasos:" -ForegroundColor Cyan
Write-Host "    1. Busca el icono de Nudge en la bandeja del sistema"
Write-Host "    2. Crea tu primer recordatorio con '+ Nuevo'"
Write-Host "    3. Las siguientes versiones se instalan solas desde la app"
Write-Host ""
