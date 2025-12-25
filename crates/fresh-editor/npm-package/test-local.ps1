# Local testing script for npm package (Windows PowerShell)

Write-Host "🧪 Testing Fresh npm package locally..." -ForegroundColor Cyan
Write-Host ""

# Get version from Cargo.toml
$VERSION = (Select-String -Path "..\Cargo.toml" -Pattern '^version\s*=\s*"([^"]+)"' | Select-Object -First 1).Matches.Groups[1].Value
Write-Host "📦 Version: $VERSION" -ForegroundColor Green
Write-Host ""

# Create test package
Write-Host "📝 Creating test package..." -ForegroundColor Yellow
Remove-Item -Recurse -Force test-package -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Path test-package | Out-Null

# Copy files
Copy-Item *.js test-package\
Copy-Item README.md test-package\

# Create package.json from template with version
$packageTemplate = Get-Content package.json.template -Raw
$packageJson = $packageTemplate -replace 'VERSION_PLACEHOLDER', $VERSION
Set-Content test-package\package.json -Value $packageJson

# Copy supporting files
Copy-Item ..\LICENSE test-package\ -ErrorAction SilentlyContinue
Copy-Item ..\CHANGELOG.md test-package\ -ErrorAction SilentlyContinue
Copy-Item ..\plugins test-package\ -Recurse -ErrorAction SilentlyContinue
Copy-Item ..\themes test-package\ -Recurse -ErrorAction SilentlyContinue

Write-Host "✅ Package created" -ForegroundColor Green
Write-Host ""

# Pack it
Push-Location test-package
npm pack | Out-Null
$packageFile = Get-ChildItem *.tgz | Select-Object -First 1
Move-Item $packageFile.Name ..\test-package.tgz -Force
Pop-Location

Write-Host "📦 Created: test-package.tgz" -ForegroundColor Green
Write-Host ""

# Test installation
Write-Host "🔧 Testing installation..." -ForegroundColor Yellow
$testDir = Join-Path $env:TEMP "fresh-test-$(Get-Random)"
New-Item -ItemType Directory -Path $testDir | Out-Null
Push-Location $testDir

$packagePath = Join-Path $PSScriptRoot "test-package.tgz"
npm install $packagePath

Write-Host ""
Write-Host "✅ Installation complete" -ForegroundColor Green
Write-Host ""

# Test binary
Write-Host "🚀 Testing binary..." -ForegroundColor Yellow
$freshPath = Join-Path (Get-Location) "node_modules\.bin\fresh.cmd"
if (Test-Path $freshPath) {
    & $freshPath --version
    Write-Host "✅ Binary works!" -ForegroundColor Green
} else {
    Write-Host "❌ Binary not found" -ForegroundColor Red
    Pop-Location
    exit 1
}

Pop-Location

Write-Host ""
Write-Host "✅ All tests passed!" -ForegroundColor Green
Write-Host "📍 Test directory: $testDir" -ForegroundColor Cyan
