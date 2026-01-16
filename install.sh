#!/bin/bash

# Knot System-wide Installer

echo "🚀 Building Knot in release mode..."
cargo build --release

if [ $? -eq 0 ]; then
    echo "✅ Build successful."
    
    # Check if /usr/local/bin exists
    if [ ! -d "/usr/local/bin" ]; then
        sudo mkdir -p /usr/local/bin
    fi

    echo "📦 Moving binary to /usr/local/bin/knot..."
    sudo cp target/release/knot /usr/local/bin/knot
    
    echo "🔒 Setting permissions..."
    sudo chmod +x /usr/local/bin/knot

    echo "✨ Done! You can now run 'knot' from any directory."
else
    echo "❌ Build failed. Please check your Rust installation."
    exit 1
fi
