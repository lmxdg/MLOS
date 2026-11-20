# MLOS

**MLOS**（Minimal Light Operating System）是一个用 Rust 从零编写的、可从光盘启动的 64 位操作系统。相比基础内核，它加入了**用户安装向导、账号密码登录、硬盘数据持久化、小游戏和实用工具**，以 ISO 镜像交付，可直接写入 U 盘或在虚拟机中启动。

## 特性

### 系统核心
- **no_std 裸机内核**：不依赖标准库，从 Multiboot2 引导头亲手构建运行环境
- **32→64 位长模式切换**：自建页表、启用 PAE/Long Mode/SSE、加载 64 位 GDT
- **VGA 文本驱动**：80×25 彩色文本输出、滚动、光标控制
- **PS/2 键盘驱动**：轮询 Scancode Set 1，支持 Shift、Caps Lock、退格
- **内存检测**：解析 Multiboot2 内存映射，统计可用内存
- **GRUB 2 引导**：打包成标准可引导 ISO（BIOS 模式）

### 用户与存储（新增）
- **首次启动安装向导**：设置用户名与密码（密码以 `*` 掩码输入）
- **登录认证**：重启后需输入用户名密码，密码用 FNV-1a 哈希存储
- **ATA PIO 硬盘驱动**：Primary Master，LBA28 读写 512 字节扇区
- **数据持久化**：用户配置写入硬盘 LBA 1，重启后自动读回（含校验和）
- **`disk` 命令**：查看硬盘与配置状态；`setup` 命令可重新运行安装向导

### 小游戏（新增）
| 命令 | 游戏 |
|------|------|
| `bomb` | 数字炸弹 —— 逼近隐藏炸弹，越近越危险，踩中即爆炸 |
| `guess` | 猜数字 —— 经典 1~100 猜数字 |
| `rps` | 石头剪刀布 —— 五局三胜对战电脑 |

### 实用工具（新增）
| 命令 | 工具 |
|------|------|
| `calc [expr]` | 四则运算计算器（支持括号，`calc` 进入交互模式）|
| `base <n>` | 进制转换（DEC/HEX/BIN/OCT）|
| `prime <n>` | 素数判断 |
| `hash <text>` | FNV-1a 哈希计算 |
| `rand [max]` | 随机数生成（RDTSC + xorshift64*）|
| `ascii` | ASCII 可打印字符表 |

基础命令：`help` / `clear` / `echo` / `version` / `about` / `whoami` / `uname` / `date` / `mem` / `color` / `banner` / `reboot` / `halt`。

## 目录结构

```
mlos/
├── Cargo.toml           # 项目配置（mlos v1.0.0, panic=abort）
├── .cargo/config.toml   # 自定义目标 + build-std
├── x86_64-os.json       # 自定义裸机编译目标
├── linker.ld            # 链接脚本（内核加载地址 2 MiB）
├── build.rs             # 链接参数 + 注入编译时间
├── src/
│   ├── boot.rs          # Multiboot2 头 + 32→64 位切换存根
│   ├── main.rs          # 内核入口、横幅、启动流程、内存解析
│   ├── ata.rs           # ATA PIO 硬盘驱动（读/写扇区）
│   ├── storage.rs       # 配置持久化（读写 LBA1 + 校验和 + FNV-1a）
│   ├── setup.rs         # 安装向导 + 登录认证
│   ├── input.rs         # 通用行输入（含密码掩码）
│   ├── rng.rs           # xorshift64* 随机数
│   ├── games.rs         # 数字炸弹 / 猜数字 / 石头剪刀布
│   ├── tools.rs         # 计算器 / 进制转换 / 素数 / 哈希 / ASCII
│   ├── vga.rs           # VGA 文本模式驱动
│   ├── keyboard.rs      # PS/2 键盘驱动
│   ├── io.rs            # I/O 端口（inb/outb/inw/outw/rdtsc）
│   └── shell.rs         # 交互式 Shell（命令分发）
├── grub.cfg             # GRUB 菜单（MLOS v1.0.0）
├── Makefile             # 一键构建/运行/清理（含虚拟硬盘）
├── MLOS.iso             # 最终可引导 ISO（成品）
├── mlos-disk.img        # 16MB 持久化虚拟硬盘（首次构建自动生成）
└── screenshots/         # QEMU 实测截图
```

## 构建

环境要求：Rust 工具链（含 `rust-src`）、GRUB（`grub-mkrescue`）、`xorriso`、`qemu-img`（硬盘镜像用）。项目用 `-Z build-std` 从源码编译 `core`，需 `RUSTC_BOOTSTRAP=1`（无需完整 nightly）。

```bash
cd mlos
make all           # 编译内核 + 生成 MLOS.iso + 创建 mlos-disk.img
```

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

## 常见问题

- **必须用 BIOS 启动**：ISO 用 GRUB i386-pc 引导，不支持纯 UEFI。
- **中文显示乱码**：VGA 文本模式（CP437 字体）不含中文字形，Shell 输出全部为 ASCII 英文。
- **QEMU 启动慢**：SeaBIOS 会尝试 iPXE 网络启动，加 `-net none` 加速。
- **想重置账号**：执行 `make clean` 会删除 `mlos-disk.img`，下次启动重新进入安装向导；或在 Shell 中运行 `setup` 重新配置。

## 实现要点

1. **32→64 位切换**：GRUB 用 Multiboot2 加载 ELF64 内核时 CPU 处于 32 位保护模式，`boot.rs` 存根建页表→PAE/LME/PG→64 位 GDT→`jmp far` 进入长模式。
2. **ATA PIO**：通过端口 0x1F0~0x1F7 用 LBA28 命令（0x20 读 / 0x30 写）访问硬盘，`0xE7` 刷缓存确保落盘。
3. **持久化布局**：配置存在 LBA 1，含魔数 `MLOS`、版本、用户名、FNV-1a 密码哈希与校验和，读取时校验防止脏数据。
4. **密码安全**：不存明文，只存 64 位哈希；输入时回显 `*`。
