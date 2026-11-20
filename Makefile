KERNEL := target/x86_64-os/release/mlos
ISODIR := iso
ISO    := MLOS.iso
DISK   := mlos-disk.img
DISKSZ := 16M

.PHONY: all kernel iso disk run debug rebuild clean

all: iso disk

boot:
	nasm -f bin boot/mbr.asm -o boot/mbr.bin
	nasm -f bin boot/stage2.asm -o boot/stage2.bin

kernel: boot
	RUSTC_BOOTSTRAP=1 cargo build --release

iso: kernel
	mkdir -p $(ISODIR)/boot/grub
	cp $(KERNEL) $(ISODIR)/boot/mlos.bin
	cp grub.cfg $(ISODIR)/boot/grub/grub.cfg
	grub-mkrescue -o $(ISO) $(ISODIR)
	@echo "==> Built $(ISO)"

# 创建 16MB 持久化虚拟硬盘（若不存在）
disk:
	@if [ ! -f $(DISK) ]; then \
		echo "==> Creating $(DISK) ($(DISKSZ))"; \
		qemu-img create -f raw $(DISK) $(DISKSZ); \
	fi

run: all
	qemu-system-x86_64 -cdrom $(ISO) -m 128M -boot d -net none -no-reboot \
		-drive file=$(DISK),format=raw,if=ide,index=0,media=disk

debug: all
	qemu-system-x86_64 -cdrom $(ISO) -m 128M -boot d -net none -no-reboot \
		-drive file=$(DISK),format=raw,if=ide,index=0,media=disk \
		-d int -D qemu.log

rebuild:
	cargo clean
	$(MAKE) all

clean:
	cargo clean
	rm -rf $(ISODIR) $(ISO) qemu.log $(DISK)
