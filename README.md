# 且慢 🛑

> **告别 AI 提前终止烦恼，过程介入交互方式，助力 AI 更加持久**

本项目 fork 自 [imhuso/cunzhi](https://github.com/imhuso/cunzhi)，主要进行了命名优化、显示md格式的数学公式及其他部分功能改进，使其更适合日常使用和分享。

## 🔄 主要改动

相比原项目，本 fork 主要做了以下调整：

- **命名优化**：将部分不雅命名替换为更友好的中文名称
  - MCP 工具名：`zhi` → `heng`
  - 设置命令：`等一下` → `等`
- **CLI 安装**：新增设置界面一键安装功能，方便直接部署到系统 PATH
- **MD数学公式**：支持md格式的数学公式显示
- **输入优化**：优化大文本输入时的性能问题

## 🌟 核心特性

- 🛑 **智能拦截**：AI 想结束时自动弹出继续选项
- 🧠 **记忆管理**：按项目存储开发规范和偏好
- 🎨 **优雅交互**：Markdown 支持、多种输入方式
- ⚡ **即装即用**：简单安装，跨平台支持

## 📸 效果预览

### 🛑 智能拦截弹窗
![且慢弹窗演示](./screenshots/popup.png)

### 🧠 数学公式显示

![数学公式显示](./screenshots/math.png)

### ⚙️ 设置管理界面
![且慢设置界面](./screenshots/settings.png)

## 🚀 安装使用

### macOS

1. 下载 [Releases](https://github.com/KerwinKoo/hengjing/releases) 中的 `.dmg` 文件
2. 将 `且慢.app` 拖入 `/Applications`
3. 打开应用，在设置 → CLI 安装中点击"一键安装"

或手动安装 CLI：
```bash
sudo ln -sf /Applications/且慢.app/Contents/MacOS/恒境 /usr/local/bin/恒境
sudo ln -sf /Applications/且慢.app/Contents/MacOS/等 /usr/local/bin/等
```

### Windows

1. 下载 [Releases](https://github.com/KerwinKoo/hengjing/releases) 中的 `continuum-cli-*-windows-x86_64.zip` 文件
2. 解压到你喜欢的目录，例如 `C:\Program Files\hengjing\`
3. 将该目录添加到系统 PATH 环境变量：
   - 右键「此电脑」→「属性」→「高级系统设置」→「环境变量」
   - 在「系统变量」中找到 `Path`，点击「编辑」→「新建」
   - 添加解压目录路径，如 `C:\Program Files\hengjing`
   - 确认保存，重启终端生效

4. 验证安装：
```powershell
# 打开 PowerShell 或 CMD
且慢.exe --version
等.exe
```

#### Windows MCP 配置

```json
{
  "mcpServers": {
    "且慢": {
      "command": "C:\\Program Files\\hengjing\\恒境.exe",
      "autoApprove": ["heng"],
      "timeout": 36000000
    }
  }
}
```

> **💡 提示**：如果已添加到 PATH，也可以直接使用 `"command": "且慢.exe"`

### Linux

1. 下载 [Releases](https://github.com/KerwinKoo/hengjing/releases) 中的 `continuum-cli-*-linux-x86_64.tar.gz` 文件
2. 解压并安装：
```bash
tar -xzf continuum-cli-*-linux-x86_64.tar.gz
sudo mv 恒境 等 hengjing /usr/local/bin/
sudo chmod +x /usr/local/bin/恒境 /usr/local/bin/等 /usr/local/bin/hengjing
```

### 配置 MCP 客户端

```json
{
  "mcpServers": {
    "且慢": {
      "command": "恒境",
      "autoApprove": ["heng"],
      "timeout": 36000000
    }
  }
}
```

### 打开设置界面

```bash
等
```

## 🔧 工具说明

- **heng**：智能代码审查交互工具
- **ji**：记忆管理工具
- **sou**：代码搜索工具（基于 ACE）
  - 📖 [详细说明](./ACEMCP.md)

## 🛠️ 本地开发

```bash
git clone https://github.com/KerwinKoo/hengjing.git
cd hengjing
pnpm install
pnpm tauri:dev
```

构建发布版：
```bash
pnpm tauri:build
```

## 🐛 故障排除

### Cursor（Remote SSH）首次弹窗停留在“保持此页面打开即可...”

现象：Cursor 第一次调用 MCP 后打开了且慢的等待页，但没有进入可输入的交互界面，导致交互卡住；断开/重连后再次调用才恢复正常。

可用绕过：
- 确保 `等`/且慢 UI 进程未在后台运行（退出后再触发 MCP，让它走“新进程 + --mcp-request 文件”路径）。
- 清理残留 IPC socket：`rm -f /tmp/hengjing-ui.sock`，再触发一次 MCP。

## 🙏 致谢

- [imhuso/cunzhi](https://github.com/imhuso/cunzhi) - 原项目
- [acemcp](https://github.com/qy527145/acemcp) - 代码搜索能力

## 📄 开源协议

MIT License
