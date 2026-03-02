#!/bin/bash

# 且慢 MCP 工具 - 最简化安装脚本
# 只需构建 qieman CLI 工具即可运行 MCP

set -e

echo "🚀 安装 且慢 MCP 工具..."

# 检查必要工具
for cmd in cargo pnpm; do
    if ! command -v "$cmd" &> /dev/null; then
        echo "❌ 请先安装 $cmd"
        exit 1
    fi
done

# 确保 pnpm 使用较新的 Node（vite 6 需要 Node >= 18）
node_major="$(node --version 2>/dev/null | sed -E 's/^v([0-9]+).*/\1/' || echo 0)"
if [[ "$node_major" -lt 18 ]]; then
    pnpm_bin="$(command -v pnpm)"
    pnpm_dir="$(dirname "$pnpm_bin")"
    if [[ -x "$pnpm_dir/node" ]]; then
        export PATH="$pnpm_dir:$PATH"
        node_major="$(node --version | sed -E 's/^v([0-9]+).*/\1/')"
    fi
fi
if [[ "$node_major" -lt 18 ]]; then
    echo "❌ Node.js 版本过低: $(node --version 2>/dev/null || true)"
    echo "   需要 Node >= 18 才能运行 pnpm/vite 构建。"
    exit 1
fi

# 构建
echo "📦 构建前端资源..."
pnpm build

echo "🔨 构建 CLI 工具..."
export CC="${CC:-/usr/bin/gcc}"
export CXX="${CXX:-/usr/bin/g++}"
cargo build --release

# 检查构建结果
if [[ ! -f "target/release/qieman" ]]; then
    echo "❌ 构建失败"
    exit 1
fi

# 安装到用户目录
BIN_DIR="$HOME/.local/bin"
mkdir -p "$BIN_DIR"

cp "target/release/qieman" "$BIN_DIR/"
chmod +x "$BIN_DIR/qieman"

echo "✅ 安装完成！CLI 工具已安装到 $BIN_DIR"

# 检查PATH
if [[ ":$PATH:" != *":$BIN_DIR:"* ]]; then
    echo ""
    echo "💡 请将以下内容添加到 ~/.bashrc 或 ~/.zshrc:"
    echo "export PATH=\"\$PATH:$BIN_DIR\""
    echo "然后运行: source ~/.bashrc"
fi

echo ""
echo "📋 使用方法："
echo "  qieman serve    - 启动 MCP 服务器"
echo "  qieman gui      - 启动设置界面"
echo ""
echo "📝 MCP 客户端配置："
echo '{"mcpServers": {"且慢": {"command": "qieman", "args": ["serve"]}}}'
