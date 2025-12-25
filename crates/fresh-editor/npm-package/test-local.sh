#!/bin/bash
# Local testing script for npm package

set -e

echo "🧪 Testing Fresh npm package locally..."
echo ""

# Get version from Cargo.toml
VERSION=$(grep '^version' ../Cargo.toml | head -1 | sed 's/.*"\(.*\)".*/\1/')
echo "📦 Version: $VERSION"
echo ""

# Create test package
echo "📝 Creating test package..."
rm -rf test-package
mkdir -p test-package

# Copy files
cp *.js test-package/
cp README.md test-package/

# Create package.json from template with version
sed "s/VERSION_PLACEHOLDER/${VERSION}/g" package.json.template > test-package/package.json

# Copy supporting files
cp ../LICENSE ../CHANGELOG.md test-package/ 2>/dev/null || true
cp -r ../plugins test-package/ 2>/dev/null || true
cp -r ../themes test-package/ 2>/dev/null || true

echo "✅ Package created"
echo ""

# Pack it
cd test-package
npm pack
PACKAGE_FILE=$(ls *.tgz)
mv "$PACKAGE_FILE" ../test-package.tgz
cd ..

echo "📦 Created: test-package.tgz"
echo ""

# Test installation
echo "🔧 Testing installation..."
TEST_DIR=$(mktemp -d)
cd "$TEST_DIR"

npm install "$OLDPWD/test-package.tgz"

echo ""
echo "✅ Installation complete"
echo ""

# Test binary
echo "🚀 Testing binary..."
if command -v fresh &> /dev/null; then
    fresh --version
    echo "✅ Binary works!"
else
    echo "❌ Binary not found in PATH"
    exit 1
fi

echo ""
echo "✅ All tests passed!"
echo "📍 Test directory: $TEST_DIR"
