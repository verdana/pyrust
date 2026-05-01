#!/bin/bash
# 打包 pyrust 供 Windows 测试（交叉编译）
set -e

PROJECT_DIR="/home/verdana/workspace/pyrust/src"
DEST_DIR="/mnt/c/Users/Verdana/Desktop/pyrust-test"
TARGET="x86_64-pc-windows-gnu"

echo "=== 打包 pyrust for Windows 测试 ==="

# 确保 Windows 目标已安装
. "$HOME/.cargo/env" 2>/dev/null || true
rustup target add "$TARGET" 2>/dev/null || true

# 构建
echo "交叉编译 Windows 二进制..."
cargo build --release --target "$TARGET"

# 清理旧目录
if [ -d "$DEST_DIR" ]; then
    echo "清理旧目录..."
    rm -rf "$DEST_DIR"
fi
mkdir -p "$DEST_DIR"

# 复制可执行文件
echo "复制可执行文件..."
cp "target/$TARGET/release/pyrust.exe" "$DEST_DIR/"

# 复制资源文件
cp assets/base.dict "$DEST_DIR/" 2>/dev/null || true
cp assets/bigram.dat "$DEST_DIR/" 2>/dev/null || true

# 复制源代码
echo "复制源代码..."
rsync -aq --exclude='target' --exclude='.git' --exclude='.omc' "$PROJECT_DIR/" "$DEST_DIR/src/"

# 创建 Windows 构建脚本
cat > "$DEST_DIR/build.ps1" << 'EOF'
# pyrust Windows 构建脚本
# 首次运行会安装 Rust（如果未安装）

Write-Host "=== pyrust Windows 构建 ===" -ForegroundColor Cyan

# 检查 Rust 是否安装
if (-not (Get-Command rustc -ErrorAction SilentlyContinue)) {
    Write-Host "Rust 未安装，正在安装..." -ForegroundColor Yellow

    # 下载并运行 rustup-init
    $rustupUrl = "https://win.rustup.rs/x86_64"
    $rustupPath = "$env:TEMP\rustup-init.exe"

    Invoke-WebRequest -Uri $rustupUrl -OutFile $rustupPath
    Write-Host "请按提示安装 Rust（选择默认选项即可）" -ForegroundColor Yellow
    Start-Process -FilePath $rustupPath -Wait

    # 刷新环境变量
    $env:Path = [System.Environment]::GetEnvironmentVariable("Path", "Machine") + ";" + [System.Environment]::GetEnvironmentVariable("Path", "User")

    Write-Host "Rust 安装完成！" -ForegroundColor Green
}

# 进入源代码目录
Set-Location "$PSScriptRoot\src"

Write-Host "构建 release 版本..." -ForegroundColor Cyan
cargo build --release

if ($LASTEXITCODE -eq 0) {
    Write-Host "构建成功！" -ForegroundColor Green

    # 复制可执行文件到桌面
    Copy-Item "target\release\pyrust.exe" "$PSScriptRoot\.." -Force

    Write-Host ""
    Write-Host "可执行文件位置: $PSScriptRoot\..\pyrust.exe" -ForegroundColor Green
    Write-Host ""
    Write-Host "运行方式: 在 CMD 或 PowerShell 中执行 .\pyrust.exe" -ForegroundColor Yellow
} else {
    Write-Host "构建失败！" -ForegroundColor Red
    exit 1
}
EOF

# 创建运行脚本
cat > "$DEST_DIR/run.bat" << 'EOF'
@echo off
cd /d "%~dp0"
pyrust.exe
pause
EOF

# 创建 README
cat > "$DEST_DIR/README.txt" << 'EOF'
pyrust Windows 测试说明
========================

1. 首次构建：
   - 右键 build.ps1 -> "使用 PowerShell 运行"
   - 或在 PowerShell 中执行: .\build.ps1
   - 脚本会自动安装 Rust（如果未安装）

2. 后续构建：
   - 直接运行 build.ps1 即可

3. 测试输入：
   - 双击 run.bat 或直接运行 pyrust.exe
   - 在命令行输入拼音（如 nihao）
   - 应该会弹出候选词窗口

4. 退出：
   - 输入 quit 退出

构建时间: TIMESTAMP_PLACEHOLDER
EOF

# 替换时间戳
sed -i "s/TIMESTAMP_PLACEHOLDER/$TIMESTAMP/" "$DEST_DIR/README.txt"

echo ""
echo "=== 打包完成 ==="
echo "位置: $DEST_DIR"
echo ""
echo "在 Windows 上:"
echo "  1. 打开桌面上的 pyrust-test 文件夹"
echo "  2. 右键 build.ps1 -> 使用 PowerShell 运行"
echo "  3. 构建完成后双击 run.bat 测试"
