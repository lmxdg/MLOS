# MLOS v1.0.1

**MLOS**（Minimal Light Operating System）是一个用 Rust 从零编写的、可从光盘启动的 64 位操作系统。v1.0.1 新增**双渲染引擎**，支持在**英文文本模式**与**中文图形模式**之间自由切换；同时具备用户安装向导、账号密码登录、硬盘数据持久化、**在线更新检测**、小游戏和实用工具，以 ISO 镜像交付，可直接写入 U 盘或在虚拟机中启动。

> **开发者：MEME-LMXDG** · GitHub：<https://github.com/lmxdg/MLOS>

## 特性

### 双渲染引擎（v1.0.1 新增）
- **两种渲染方式，随时切换**：Shell 中执行 `render text` / `render gfx` 自由切换
  - **文本模式（text）**：经典 VGA 文本 80×25，CP437 字符，用于英文界面
  - **图形模式（gfx）**：VGA 模式 13h（320×200×256 色）线性帧缓冲，配合内嵌 **16×16 中文点阵字库**，UTF-8 解码渲染，界面自动切换为中文
- **统一输出层**：`print!/println!` 经 `Console` trait 分发到当前后端，所有模块无需关心渲染方式；`tprint!/tprintln!/tr!` 双语宏按模式选择中英文文案
- **寄存器快照恢复**：进入图形模式前保存 VGA 寄存器，切回文本时精确还原，保证两种模式来回切换显示正常
- 字库由 Noto Sans CJK SC 字体离线生成（`src/font.rs`，618 个字形）

### 系统核心
- **no_std 裸机内核**：不依赖标准库，从 Multiboot2 引导头亲手构建运行环境
- **32→64 位长模式切换**：自建页表、启用 PAE/Long Mode/SSE、加载 64 位 GDT
- **PS/2 键盘驱动**：轮询 Scancode Set 1，支持 Shift、Caps Lock、退格
- **内存检测**：解析 Multiboot2 内存映射，统计可用内存
- **GRUB 2 引导**：打包成标准可引导 ISO（BIOS 模式）

### 用户与存储
- **首次启动安装向导**：设置用户名与密码（密码以 `*` 掩码输入）
- **登录认证**：重启后需输入用户名密码，密码用 FNV-1a 哈希存储
- **ATA PIO 硬盘驱动**：Primary Master，LBA28 读写 512 字节扇区
- **数据持久化**：用户配置写入硬盘 LBA 1，重启后自动读回（含校验和）
- **`disk` 命令**：查看硬盘与配置状态；`setup` 命令可重新运行安装向导

### 在线更新检测（v1.0.1 新增）
- **`update` 命令**：读取硬盘上的版本信息扇区，与本地版本做语义版本比较，报告「已是最新 / 发现新版本 / 未获取到信息」
- **主机桥接方案**：内核运行于无网络协议栈的裸机环境，由 `tools/check_update.sh` 在宿主机通过 GitHub Releases API 拉取 `lmxdg/MLOS` 最新版本，写入硬盘 LBA 3
- 用法见下方「在线更新检测」章节

### 小游戏
| 命令 | 游戏 |
|------|------|
| `bomb` | 数字炸弹 —— 逼近隐藏炸弹，越近越危险，踩中即爆炸 |
| `guess` | 猜数字 —— 经典 1~100 猜数字 |
| `rps` | 石头剪刀布 —— 五局三胜对战电脑 |

### 实用工具
| 命令 | 工具 |
|------|------|
| `calc [expr]` | 四则运算计算器（支持括号，`calc` 进入交互模式）|
| `base <n>` | 进制转换（DEC/HEX/BIN/OCT）|
| `prime <n>` | 素数判断 |
| `hash <text>` | FNV-1a 哈希计算 |
| `rand [max]` | 随机数生成（RDTSC + xorshift64*）|
| `ascii` | ASCII 可打印字符表 |

基础命令：`help` / `clear` / `echo` / `version` / `about` / `whoami` / `uname` / `date` / `mem` / `color` / `banner` / `render` / `disk` / `update` / `setup` / `reboot` / `halt`。

## 双渲染切换用法

登录进入 Shell 后：

```
render gfx     # 切换到中文图形模式（点阵中文界面）
render text    # 切回英文文本模式
```

两种模式的界面文案会随渲染方式自动切换为中文/英文，可反复来回切换。

## 在线更新检测

MLOS 内核是 no_std 裸机程序、不带网络协议栈，因此采用「主机拉取 + 硬盘桥接」的方式实现联网更新检测：

**第一步 — 在宿主机拉取最新版本信息**（需要 `curl` 和 `python3`）：

```bash
./tools/check_update.sh            # 默认写入 ./mlos-disk.img
./tools/check_update.sh /path/to/disk.img   # 指定镜像
```

脚本会请求 `https://api.github.com/repos/lmxdg/MLOS/releases/latest`，取回最新 tag（如 `v1.0.2`），写入硬盘 LBA 3 扇区（魔数 `UPDT` + 版本号 + 校验和）。若暂时没有 Release 或处于离线状态，会自动回退读取 `Cargo.toml` 中的版本。

**第二步 — 在 MLOS 中检测**：

```
update        # 读取 LBA3，与本地 1.0.1 比较并显示结果
```

结果分三种：`已是最新版本`、`发现新版本 X.Y.Z（附下载地址）`、`未获取到信息（提示先在主机运行脚本）`。执行 `render gfx` 后运行 `update` 可看到中文界面。

> 说明：若要把检测功能用于真实物理硬盘，请在主机上把版本扇区写到对应设备的 LBA 3（注意不要覆盖 MBR 与 LBA1 配置区）。

## 目录结构

```
mlos/
├── Cargo.toml           # 项目配置（mlos v1.0.1, panic=abort）
├── .cargo/config.toml   # 自定义目标 + build-std
├── x86_64-os.json       # 自定义裸机编译目标
├── linker.ld            # 链接脚本（内核加载地址 2 MiB）
├── build.rs             # 链接参数 + 注入编译时间
├── src/
│   ├── boot.rs          # Multiboot2 头 + 32→64 位切换存根
│   ├── main.rs          # 内核入口、横幅、启动流程、内存解析
│   ├── vga.rs           # 统一终端层 + Console trait + 双语宏
│   ├── vga_text.rs      # 渲染后端一：VGA 文本模式
│   ├── gfx.rs           # 渲染后端二：VGA 图形模式 + 寄存器快照
│   ├── font.rs          # 16×16 中文点阵字库（离线生成）
│   ├── ata.rs           # ATA PIO 硬盘驱动（读/写扇区）
│   ├── storage.rs       # 配置持久化（读写 LBA1 + 校验和 + FNV-1a）
│   ├── setup.rs         # 安装向导 + 登录认证
│   ├── input.rs         # 通用行输入（含密码掩码）
│   ├── rng.rs           # xorshift64* 随机数
│   ├── update.rs        # 在线更新检测（读 LBA3 + 版本比较）
│   ├── games.rs         # 数字炸弹 / 猜数字 / 石头剪刀布
│   ├── tools.rs         # 计算器 / 进制转换 / 素数 / 哈希 / ASCII
│   ├── keyboard.rs      # PS/2 键盘驱动
│   ├── io.rs            # I/O 端口（inb/outb/inw/outw/rdtsc）
│   └── shell.rs         # 交互式 Shell（命令分发）
├── tools/
│   └── check_update.sh  # 主机侧：拉取 GitHub 最新版本写入 LBA3
├── grub.cfg             # GRUB 菜单（MLOS v1.0.1）
├── Makefile             # 一键构建/运行/清理（含虚拟硬盘）
├── MLOS.iso             # 最终可引导 ISO（成品）
├── mlos-disk.img        # 16MB 持久化虚拟硬盘（首次构建自动生成）
└── screenshots/         # QEMU 实测截图（见下方）
```

## 构建

环境要求：Rust 工具链（含 `rust-src`）、GRUB（`grub-mkrescue`）、`xorriso`、`qemu-img`（硬盘镜像用）。项目用 `-Z build-std` 从源码编译 `core`，需 `RUSTC_BOOTSTRAP=1`（无需完整 nightly）。

```bash
cd mlos
make all           # 编译内核 + 生成 MLOS.iso + 创建 mlos-disk.img
```

> 中文字库 `src/font.rs` 已随源码提交，构建无需重新生成。若需重新生成：用 Noto Sans CJK SC 字体把所需字符渲染为 16×16 单色点阵（每行一个 u16，bit15..0 对应左到右像素）。

## 运行

**QEMU（推荐，带持久化硬盘）：**

```bash
make run
# 等价于：
qemu-system-x86_64 -cdrom MLOS.iso -m 128M -boot d -net none \
    -drive file=mlos-disk.img,format=raw,if=ide,index=0,media=disk
```

首次启动会进入**安装向导**：依次设置用户名、密码、确认密码。配置写入硬盘后自动进入登录。之后每次启动都会要求登录——这就是数据持久化的体现。

**写入 U 盘（Linux）：**

```bash
sudo dd if=MLOS.iso of=/dev/sdX bs=4M status=progress && sync
```

> 警告：`/dev/sdX` 请替换成你的 U 盘设备名，写错会破坏数据。注意 U 盘启动需要额外提供一块硬盘/分区用于保存数据。

**虚拟机：** 在 VirtualBox / VMware 中新建虚拟机，挂载 `MLOS.iso`，选择 **BIOS（非 UEFI）** 启动，并添加一块硬盘用于持久化。

## 系统截图

所有截图均为 QEMU 实机运行结果，位于 `screenshots/`。

### 中文图形模式（render gfx）

| 主界面 / 命令 | 截图 |
|------|------|
| 中文主界面 banner | ![](screenshots/cn_main.png) |
| help 命令列表 | ![](screenshots/cn_help.png) |
| about 关于系统 | ![](screenshots/cn_about.png) |
| version / date | ![](screenshots/cn_version.png) |

### 中文小游戏

| 游戏 | 截图 |
|------|------|
| 数字炸弹 bomb | ![](screenshots/cn_game_bomb.png) |
| 猜数字 guess | ![](screenshots/cn_game_guess.png) |
| 石头剪刀布 rps | ![](screenshots/cn_game_rps.png) |

### 中文实用工具

| 工具 | 截图 |
|------|------|
| 工具菜单 tools | ![](screenshots/cn_tools_menu.png) |
| 计算器 calc | ![](screenshots/cn_calc.png) |
| 进制转换 base | ![](screenshots/cn_base.png) |
| 素数判断 prime | ![](screenshots/cn_prime.png) |
| 随机数 rand | ![](screenshots/cn_rand.png) |
| 硬盘状态 disk | ![](screenshots/cn_disk.png) |
| 系统更新检测 update | ![](screenshots/cn_update.png) |

### 英文文本模式（render text）

| 界面 | 截图 |
|------|------|
| Shell 首屏 | ![](screenshots/en_text_shell.png) |
| help 命令列表 | ![](screenshots/en_text_help.png) |
| about 关于系统 | ![](screenshots/en_text_about.png) |

### 安装与登录

| 阶段 | 截图 |
|------|------|
| 首次安装向导 | ![](screenshots/en_setup_wizard.png) |
| 安装完成进入系统 | ![](screenshots/en_setup_complete.png) |
| 重启后登录界面 | ![](screenshots/en_login.png) |

## 常见问题

- **必须用 BIOS 启动**：ISO 用 GRUB i386-pc 引导，不支持纯 UEFI。
- **中文显示**：默认英文文本模式；在 Shell 执行 `render gfx` 即可切换到中文图形模式再执行 `render text` 切回。
- **图形模式行长限制**：图形模式为 320×200 像素、16×16 字符格，即 20 列 × 12 行，较长的行会自动换行。
- **QEMU 启动慢**：SeaBIOS 会尝试 iPXE 网络启动，加 `-net none` 加速。
- **想重置账号**：执行 `make clean` 会删除 `mlos-disk.img`，下次启动重新进入安装向导；或在 Shell 中运行 `setup` 重新配置。

## 实现要点

1. **32→64 位切换**：GRUB 用 Multiboot2 加载 ELF64 内核时 CPU 处于 32 位保护模式，`boot.rs` 存根建页表→PAE/LME/PG→64 位 GDT→`jmp far` 进入长模式。
2. **双渲染引擎**：`vga.rs` 定义 `Console` trait 并用 `with_writer` 按 `RenderMode` 分发；文本后端 `vga_text.rs` 写 0xB8000，图形后端 `gfx.rs` 通过直接写 VGA 寄存器进入模式 13h，向 0xA0000 线性帧缓冲按点阵绘制，UTF-8 解码后查 `font.rs` 字库。
3. **寄存器快照**：单纯写标准模式表无法完美还原 BIOS/GRUB 初始文本模式，故进入图形前用 `read_state()` 保存全部 VGA 寄存器，切回时 `restore_state()` 原样写回，避免字符错位。
4. **ATA PIO**：通过端口 0x1F0~0x1F7 用 LBA28 命令（0x20 读 / 0x30 写）访问硬盘，`0xE7` 刷缓存确保落盘。
5. **持久化布局**：配置存在 LBA 1，含魔数 `MLOS`、版本、用户名、FNV-1a 密码哈希与校验和，读取时校验防止脏数据。
6. **密码安全**：不存明文，只存 64 位哈希；输入时回显 `*`。
7. **更新检测**：内核无网络栈，版本信息经主机脚本写入 LBA 3（魔数 `UPDT` + 版本号 + 校验和）；`update.rs` 读回后按 (major, minor, patch) 与本地版本比较，报告三种状态。
