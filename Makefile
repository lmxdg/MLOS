KERNEL := target/x86_64-os/release/mlos
ISO    := MLOS.iso
DISK   := mlos-disk.img
DISKSZ := 32M
NASM   ?= nasm
PYTHON ?= python3
QEMU   ?= qemu-system-x86_64

.PHONY: all boot kernel iso disk run run-vnc debug rebuild clean

all: iso disk

boot:
	$(NASM) -f bin boot/mbr.asm -o boot/mbr.bin
	$(NASM) -f bin -I boot/ boot/stage2.asm -o boot/stage2.bin
	$(NASM) -f bin -I boot/ boot/isoboot.asm -o boot/isoboot.bin

kernel: boot
	RUSTC_BOOTSTRAP=1 cargo build --release -Zjson-target-spec

iso: kernel
	$(PYTHON) tools/mkiso.py $(KERNEL) boot/isoboot.bin $(ISO)

disk:
	@if [ ! -f $(DISK) ]; then \
		echo "==> Creating $(DISK) ($(DISKSZ))"; \
		if command -v qemu-img >/dev/null 2>&1; then \
			qemu-img create -f raw $(DISK) $(DISKSZ); \
		else \
			$(PYTHON) -c "open('$(DISK)','wb').truncate(32*1024*1024)"; \
		fi; \
	fi

run: all
	$(QEMU) -cdrom $(ISO) -m 128M -boot d -net none -no-reboot \
		-vga std \
		-drive file=$(DISK),format=raw,if=ide,index=0,media=disk

run-vnc: all
	$(QEMU) -cdrom $(ISO) -m 128M -boot d -net none -no-reboot \
		-vga std -display vnc=:0 \
		-drive file=$(DISK),format=raw,if=ide,index=0,media=disk

debug: all
	$(QEMU) -cdrom $(ISO) -m 128M -boot d -net none -no-reboot \
		-vga std -d int -D qemu.log \
		-drive file=$(DISK),format=raw,if=ide,index=0,media=disk

rebuild:
	cargo clean
	$(MAKE) all

clean:
	cargo clean
	rm -rf iso $(ISO) qemu.log $(DISK) boot/isoboot.bin
