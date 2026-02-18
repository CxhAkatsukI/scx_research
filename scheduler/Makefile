# Compiler and tool settings
CLANG ?= clang # ?= refers to a variable that can be overridden by environment variables or command line arguments
BPFTOOL ?= bpftool

# BPF Compilation flags:
# -g: generate debug info
# -O2: enable optimization
# -target bpf: generate BPF bytecode
# -I.: target the current directory for header files
CFLAGS := -g -O2 -target bpf -I.

# Target output file
TARGET := simple.bpf.o
SRC := scx_simple.bpf.c

.PHONY: all clean

all: $(TARGET)

# Generate vmlinux.h from the kernel's BTF info.
vmlinux.h:
	@echo "Generating vmlinux.h..."
	$(BPFTOOL) btf dump file /sys/kernel/btf/vmlinux format c > vmlinux.h

# Compile the BPF program
$(TARGET): $(SRC) vmlinux.h
	@echo "Compiling BPF scheduler..."
	$(CLANG) $(CFLAGS) -c $(SRC) -o $(TARGET)

clean:
	rm -f $(TARGET) vmlinux.h