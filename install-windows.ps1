# 且慢 Windows 安装脚本

param(
    [switch]$BuildOnly = $false
)

$ErrorActionPreference = "Stop"

Write-Host "🚀 开始安装 且慢 (Windows)..." -ForegroundColor Green

# 检查必要的工具
function Test-Command {
    param($Command)
    try {
        Get-Command $Command -ErrorAction Stop | Out-Null
        return $true
    }
    catch {
        return $false
    }
}

Write-Host "🔧 检查必要工具..." -ForegroundColor Yellow

if (-not (Test-Command "cargo")) {
    Write-Host "❌ 错误: 未找到 cargo 命令" -ForegroundColor Red
    Write-Host "请先安装 Rust: https://rustup.rs/" -ForegroundColor Red
    exit 1
}

if (-not (Test-Command "pnpm")) {
    Write-Host "❌ 错误: 未找到 pnpm 命令" -ForegroundColor Red
    Write-Host "请先安装 pnpm: npm install -g pnpm" -ForegroundColor Red
    exit 1
}

# 构建前端
Write-Host "📦 构建前端资源..." -ForegroundColor Yellow
pnpm build

# 构建二进制文件
Write-Host "🔨 构建二进制文件..." -ForegroundColor Yellow
cargo build --release

# 检查构建结果 (恒境 为 MCP 服务器，等 为 GUI)

$DengPath = "target\release\等.exe"
$HengPath = "target\release\恒境.exe"
if (-not (Test-Path 
    Write-Host "❌ 二进制文件构建失败，需要: 等.exe, 恒境.exe" -ForegroundColor Red
    exit 1
}

Write-Host "✅ 二进制文件构建成功" -ForegroundColor Green

# 如果只构建不安装，则在这里退出
if ($BuildOnly) {
    Write-Host ""
    Write-Host "🎉 且慢 构建完成！" -ForegroundColor Green
    Write-Host ""
    Write-Host "📋 二进制文件位置: target\release\" -ForegroundColor Cyan
    Write-Host ""
    Write-Host "如需安装，请重新运行脚本而不使用 -BuildOnly 参数。"
    exit 0
}

# 创建安装目录
$LocalAppData = $env:LOCALAPPDATA
$InstallDir = "$LocalAppData\且慢"
$BinDir = "$InstallDir\bin"

Write-Host "📁 创建安装目录: $InstallDir" -ForegroundColor Yellow
New-Item -ItemType Directory -Path $BinDir -Force | Out-Null

# 复制二进制文件
$MainExe = "$BinDir\恒境.exe"
$UiExe = "$BinDir\等.exe"
$McpExe = "$BinDir\恒境.exe"

Write-Host "📋 安装二进制文件..." -ForegroundColor Yellow
Copy-Item 
Copy-Item $DengPath $UiExe -Force
Copy-Item $HengPath $McpExe -Force

Write-Host "✅ 二进制文件已安装到: $BinDir" -ForegroundColor Green

# 图标已移除，不再需要复制

# 检查PATH环境变量
$CurrentPath = [Environment]::GetEnvironmentVariable("PATH", "User")
if ($CurrentPath -notlike "*$BinDir*") {
    Write-Host "🔧 添加到用户 PATH 环境变量..." -ForegroundColor Yellow
    
    try {
        $NewPath = if ($CurrentPath) { "$CurrentPath;$BinDir" } else { $BinDir }
        [Environment]::SetEnvironmentVariable("PATH", $NewPath, "User")
        Write-Host "✅ 已添加到 PATH: $BinDir" -ForegroundColor Green
        Write-Host "💡 请重新启动命令提示符或 PowerShell 以使 PATH 生效" -ForegroundColor Cyan
    }
    catch {
        Write-Host "⚠️  无法自动添加到 PATH，请手动添加: $BinDir" -ForegroundColor Yellow
    }
} else {
    Write-Host "✅ PATH 已包含安装目录" -ForegroundColor Green
}

# 创建开始菜单快捷方式
$StartMenuDir = "$env:APPDATA\Microsoft\Windows\Start Menu\Programs"
$ShortcutPath = "$StartMenuDir\且慢.lnk"

try {
    $WshShell = New-Object -ComObject WScript.Shell
    $Shortcut = $WshShell.CreateShortcut($ShortcutPath)
    $Shortcut.TargetPath = $MainExe
    $Shortcut.WorkingDirectory = $InstallDir
    $Shortcut.Description = "且慢 - AI 交互确认助手，助力AI持续交互"
    # 图标已移除，使用默认图标
    $Shortcut.Save()
    Write-Host "✅ 开始菜单快捷方式已创建" -ForegroundColor Green
}
catch {
    Write-Host "⚠️  无法创建开始菜单快捷方式: $($_.Exception.Message)" -ForegroundColor Yellow
}

Write-Host ""
Write-Host "