# 且慢 MCP 工具安装指南

## 快速安装

### 方式一：使用安装脚本（推荐）

```bash
git clone https://github.com/PandAddKai/hengjing.git
cd hengjing

chmod +x install.sh
./install.sh
```

### 方式二：下载预编译版本

从 [Releases](https://github.com/PandAddKai/hengjing/releases) 页面下载对应平台的预编译版本：

- **Linux**: `continuum-cli-*-linux-x86_64.tar.gz`
- **macOS**: `continuum-cli-*-macos-universal.tar.gz`
- **Windows**: `continuum-cli-*-windows-x86_64.zip`

#### 安装步骤：

1. 下载对应平台的压缩包
2. 解压到任意目录
3. 将解压目录添加到 PATH 环境变量

```bash
# Linux/macOS 示例
tar -xzf continuum-cli-*-linux-x86_64.tar.gz
cp 等 恒境 ~/.local/bin/
```

## 验证安装

```bash
恒境 --help
```

## MCP 客户端配置

将以下配置添加到您的 MCP 客户端配置文件中：

```json
{
  "mcpServers": {
    "且慢": {
      "command": "恒境"
    }
  }
}
```

## 使用方法

### 统一入口（推荐）
```bash
```

### 兼容命令
```bash
```

## 工具说明

- **恒境**: MCP 服务器（向后兼容）
- **等**: GUI 设置界面（向后兼容）

## 系统要求

- **Linux**: x86_64 架构
- **macOS**: 10.15+ (支持 Intel 和 Apple Silicon)
- **Windows**: Windows 10+ x86_64

## 故障排除

### 权限问题
```bash
chmod +x 等 恒境
```

### PATH 问题
确保安装目录已添加到 PATH 环境变量中。

### 依赖问题
三个 CLI 工具必须在同一目录下才能正常工作。

## 开发者安装

```bash
# 安装依赖
cargo --version  # 需要 Rust 1.70+
pnpm --version   # 需要 pnpm

# 构建
pnpm install
pnpm build
cargo build --release

# 安装
cp target/release/{等,恒境} ~/.local/bin/
```

## 更新

### 使用预编译版本
重新下载最新版本并替换旧文件。

### 使用源码
```bash
git pull
pnpm build
cargo build --release
cp target/release/{等,恒境} ~/.local/bin/
```
