BUILD_DIR ?= build
CC ?= cc
AR ?= ar
NASM ?= nasm
CFLAGS ?= -O3 -g
CPPFLAGS ?=
PKG_CONFIG ?= pkg-config
CHECKASM_CFLAGS := $(shell $(PKG_CONFIG) --cflags checkasm 2>/dev/null || \
	test ! -f /usr/local/include/checkasm/checkasm.h || echo -I/usr/local/include -pthread)
CHECKASM_LIBS := $(shell $(PKG_CONFIG) --libs checkasm 2>/dev/null || \
	test ! -f /usr/local/lib/libcheckasm.a || echo -L/usr/local/lib -lcheckasm -pthread -lrt -ldl -lm)

LIB_SOURCES := src/ffvp8.c src/vp8.c src/vp8dsp.c src/vp56rac.c \
	src/h264pred.c src/compat.c
LIB_OBJECTS := $(LIB_SOURCES:src/%.c=$(BUILD_DIR)/%.o)
LIB_DEPS := $(LIB_OBJECTS:.o=.d)

MACHINE ?= $(shell uname -m)
ifneq ($(filter x86_64 amd64 i386 i486 i586 i686 x86,$(MACHINE)),)
ifneq ($(filter x86_64 amd64,$(MACHINE)),)
X86_FORMAT := elf64
X86_64 := 1
X86_32 := 0
else
X86_FORMAT := elf32
X86_64 := 0
X86_32 := 1
endif
ifneq ($(shell command -v $(NASM) 2>/dev/null),)
CPPFLAGS += -DFFVP8_ENABLE_X86_SIMD=1 -DFFVP8_H264PRED_X86=1
LIB_OBJECTS += $(BUILD_DIR)/vp8dsp_x86.o $(BUILD_DIR)/vp8dsp_loopfilter_x86.o \
	$(BUILD_DIR)/vp8dsp_init_x86.o $(BUILD_DIR)/h264pred_x86.o \
	$(BUILD_DIR)/h264pred_init_x86.o $(BUILD_DIR)/videodsp_x86.o \
	$(BUILD_DIR)/videodsp_init_x86.o
endif
endif
ifneq ($(filter armv6% armv7% armv8% armhf,$(MACHINE)),)
CPPFLAGS += -DFFVP8_H264PRED_ARM=1
LIB_OBJECTS += $(BUILD_DIR)/vp8_armv6.o $(BUILD_DIR)/vp8dsp_armv6.o \
	$(BUILD_DIR)/vp8dsp_neon.o $(BUILD_DIR)/vp8dsp_init_arm.o \
	$(BUILD_DIR)/vp8dsp_init_armv6.o $(BUILD_DIR)/vp8dsp_init_neon.o \
	$(BUILD_DIR)/h264pred_arm.o $(BUILD_DIR)/h264pred_init_arm.o \
	$(BUILD_DIR)/videodsp_arm.o $(BUILD_DIR)/videodsp_init_arm.o
endif
ifneq ($(filter aarch64 arm64,$(MACHINE)),)
CPPFLAGS += -DFFVP8_H264PRED_AARCH64=1
LIB_OBJECTS += $(BUILD_DIR)/vp8dsp_aarch64.o $(BUILD_DIR)/vp8dsp_init_aarch64.o \
	$(BUILD_DIR)/h264pred_aarch64.o $(BUILD_DIR)/h264pred_init_aarch64.o \
	$(BUILD_DIR)/videodsp_aarch64.o $(BUILD_DIR)/videodsp_init_aarch64.o
endif

.PHONY: all clean test checkasm
all: $(BUILD_DIR)/libffvp8.a $(BUILD_DIR)/wpd

$(BUILD_DIR):
	mkdir -p $@

$(BUILD_DIR)/%.o: src/%.c | $(BUILD_DIR)
	$(CC) $(CPPFLAGS) $(CFLAGS) -std=c11 -Wall -Wextra \
		-Wno-unused-parameter -Wno-maybe-uninitialized -Wno-parentheses \
		-MMD -MP -Iinclude -Isrc -c $< -o $@

$(BUILD_DIR)/vp8dsp_x86.o: src/x86/vp8dsp.asm src/asm/x86/x86inc.asm src/asm/x86/x86util.asm | $(BUILD_DIR)
	$(NASM) -w-label-orphan -w-implicit-abs-deprecated -f $(X86_FORMAT) -DPIC=1 \
		-DHAVE_ALIGNED_STACK=1 -DHAVE_X86_SSE2AVX=0 -DHAVE_AVX2_EXTERNAL=1 \
		-DARCH_X86_64=$(X86_64) -DARCH_X86_32=$(X86_32) -I src/ $< -o $@

$(BUILD_DIR)/vp8dsp_loopfilter_x86.o: src/x86/vp8dsp_loopfilter.asm src/asm/x86/x86inc.asm src/asm/x86/x86util.asm | $(BUILD_DIR)
	$(NASM) -w-label-orphan -w-implicit-abs-deprecated -f $(X86_FORMAT) -DPIC=1 \
		-DHAVE_ALIGNED_STACK=1 -DHAVE_X86_SSE2AVX=0 \
		-DARCH_X86_64=$(X86_64) -DARCH_X86_32=$(X86_32) -I src/ $< -o $@

$(BUILD_DIR)/vp8dsp_init_x86.o: src/x86/vp8dsp_init.c | $(BUILD_DIR)
	$(CC) $(CPPFLAGS) $(CFLAGS) -std=c11 -Iinclude -Isrc \
		-DHAVE_YASM=1 -DARCH_X86_32=$(X86_32) -c $< -o $@

$(BUILD_DIR)/h264pred_x86.o: src/x86/h264_intrapred.asm src/asm/x86/x86inc.asm src/asm/x86/x86util.asm | $(BUILD_DIR)
	$(NASM) -w-label-orphan -w-implicit-abs-deprecated -f $(X86_FORMAT) -DPIC=1 \
		-DHAVE_ALIGNED_STACK=1 -DHAVE_X86_SSE2AVX=0 -DHAVE_AVX2_EXTERNAL=1 \
		-DARCH_X86_64=$(X86_64) -DARCH_X86_32=$(X86_32) -I src/ $< -o $@

$(BUILD_DIR)/h264pred_init_x86.o: src/x86/h264pred_init.c | $(BUILD_DIR)
	$(CC) $(CPPFLAGS) $(CFLAGS) -std=c11 -Iinclude -Isrc \
		-c $< -o $@

$(BUILD_DIR)/videodsp_x86.o: src/x86/videodsp.asm src/asm/x86/x86inc.asm src/asm/x86/x86util.asm | $(BUILD_DIR)
	$(NASM) -w-label-orphan -w-implicit-abs-deprecated -f $(X86_FORMAT) -DPIC=1 \
		-DHAVE_ALIGNED_STACK=1 -DHAVE_X86_SSE2AVX=0 -DHAVE_AVX2_EXTERNAL=1 \
		-DARCH_X86_64=$(X86_64) -DARCH_X86_32=$(X86_32) -I src/ $< -o $@

$(BUILD_DIR)/videodsp_init_x86.o: src/x86/videodsp_init.c | $(BUILD_DIR)
	$(CC) $(CPPFLAGS) $(CFLAGS) -std=c11 -Iinclude -Isrc \
		-DHAVE_AVX2_EXTERNAL=1 -c $< -o $@

$(BUILD_DIR)/vp8_armv6.o: src/arm/vp8_armv6.S src/arm/asm.S | $(BUILD_DIR)
	$(CC) $(CPPFLAGS) $(CFLAGS) -Isrc/arm -Isrc -c $< -o $@

$(BUILD_DIR)/vp8dsp_armv6.o: src/arm/vp8dsp_armv6.S src/arm/asm.S | $(BUILD_DIR)
	$(CC) $(CPPFLAGS) $(CFLAGS) -Isrc/arm -Isrc -c $< -o $@

$(BUILD_DIR)/vp8dsp_neon.o: src/arm/vp8dsp_neon.S src/arm/asm.S src/arm/neon.S | $(BUILD_DIR)
	$(CC) $(CPPFLAGS) $(CFLAGS) -DFFVP8_ARM_NEON_ASM=1 \
		-Isrc/arm -Isrc -c $< -o $@

$(BUILD_DIR)/vp8dsp_init_arm.o: src/arm/vp8dsp_init_arm.c | $(BUILD_DIR)
	$(CC) $(CPPFLAGS) $(CFLAGS) -std=c11 -Isrc/arm -Isrc -c $< -o $@

$(BUILD_DIR)/vp8dsp_init_armv6.o: src/arm/vp8dsp_init_armv6.c | $(BUILD_DIR)
	$(CC) $(CPPFLAGS) $(CFLAGS) -std=c11 -Isrc/arm -Isrc -c $< -o $@

$(BUILD_DIR)/vp8dsp_init_neon.o: src/arm/vp8dsp_init_neon.c | $(BUILD_DIR)
	$(CC) $(CPPFLAGS) $(CFLAGS) -std=c11 -Isrc/arm -Isrc -c $< -o $@

$(BUILD_DIR)/h264pred_arm.o: src/arm/h264pred_neon.S src/arm/asm.S src/arm/neon.S | $(BUILD_DIR)
	$(CC) $(CPPFLAGS) $(CFLAGS) -DFFVP8_ARM_NEON_ASM=1 \
		-Isrc/arm -Isrc -c $< -o $@

$(BUILD_DIR)/h264pred_init_arm.o: src/arm/h264pred_init.c | $(BUILD_DIR)
	$(CC) $(CPPFLAGS) $(CFLAGS) -std=c11 -Isrc/arm -Isrc -c $< -o $@

$(BUILD_DIR)/videodsp_arm.o: src/arm/videodsp_armv5te.S src/arm/asm.S | $(BUILD_DIR)
	$(CC) $(CPPFLAGS) $(CFLAGS) -Isrc/arm -Isrc -c $< -o $@

$(BUILD_DIR)/videodsp_init_arm.o: src/arm/videodsp_init.c | $(BUILD_DIR)
	$(CC) $(CPPFLAGS) $(CFLAGS) -std=c11 -Isrc/arm -Isrc -c $< -o $@

$(BUILD_DIR)/vp8dsp_aarch64.o: src/aarch64/vp8dsp_neon.S src/aarch64/asm.S src/aarch64/neon.S src/aarch64/config.h | $(BUILD_DIR)
	$(CC) $(CPPFLAGS) $(CFLAGS) -Isrc/aarch64 -Isrc -c $< -o $@

$(BUILD_DIR)/vp8dsp_init_aarch64.o: src/aarch64/vp8dsp_init_aarch64.c | $(BUILD_DIR)
	$(CC) $(CPPFLAGS) $(CFLAGS) -std=c11 -Isrc/aarch64 -Isrc -c $< -o $@

$(BUILD_DIR)/h264pred_aarch64.o: src/aarch64/h264pred_neon.S src/aarch64/asm.S src/aarch64/neon.S src/aarch64/config.h | $(BUILD_DIR)
	$(CC) $(CPPFLAGS) $(CFLAGS) -Isrc/aarch64 -Isrc -c $< -o $@

$(BUILD_DIR)/h264pred_init_aarch64.o: src/aarch64/h264pred_init.c | $(BUILD_DIR)
	$(CC) $(CPPFLAGS) $(CFLAGS) -std=c11 -Isrc/aarch64 -Isrc -c $< -o $@

$(BUILD_DIR)/videodsp_aarch64.o: src/aarch64/videodsp.S src/aarch64/asm.S src/aarch64/config.h | $(BUILD_DIR)
	$(CC) $(CPPFLAGS) $(CFLAGS) -Isrc/aarch64 -Isrc -c $< -o $@

$(BUILD_DIR)/videodsp_init_aarch64.o: src/aarch64/videodsp_init.c | $(BUILD_DIR)
	$(CC) $(CPPFLAGS) $(CFLAGS) -std=c11 -Isrc/aarch64 -Isrc -c $< -o $@

$(BUILD_DIR)/libffvp8.a: $(LIB_OBJECTS)
	$(AR) rcs $@ $^

$(BUILD_DIR)/wpd: tools/wpd.c include/ffvp8.h $(BUILD_DIR)/libffvp8.a
	$(CC) $(CPPFLAGS) $(CFLAGS) -std=c11 -Iinclude $< $(BUILD_DIR)/libffvp8.a -o $@

$(BUILD_DIR)/checkasm: tests/checkasm/main.c tests/checkasm/vp8dsp.c \
		tests/checkasm/h264pred.c tests/checkasm/videodsp.c \
		tests/checkasm/checkasm.h $(BUILD_DIR)/libffvp8.a
	@test -n "$(CHECKASM_LIBS)" || { echo "checkasm development files not found (pkg-config checkasm)"; exit 1; }
	$(CC) $(CPPFLAGS) $(CFLAGS) -std=c11 -Wall -Wextra \
		-Wno-unused-parameter -Wno-parentheses -Iinclude -Isrc -Itests/checkasm \
		$(CHECKASM_CFLAGS) tests/checkasm/main.c tests/checkasm/vp8dsp.c \
		tests/checkasm/h264pred.c tests/checkasm/videodsp.c \
		$(BUILD_DIR)/libffvp8.a $(CHECKASM_LIBS) -o $@

checkasm: $(BUILD_DIR)/checkasm
	$(BUILD_DIR)/checkasm

test: all checkasm
	$(BUILD_DIR)/wpd i.ivf $(BUILD_DIR)/decoded.y4m
	ffmpeg -hide_banner -i i.ivf -i $(BUILD_DIR)/decoded.y4m \
		-lavfi "ssim=shortest=1" -f null - 2>&1 | tee $(BUILD_DIR)/ssim.log
	grep -F "SSIM Y:1.000000 (inf) U:1.000000 (inf) V:1.000000 (inf) All:1.000000 (inf)" \
		$(BUILD_DIR)/ssim.log

clean:
	$(RM) -r $(BUILD_DIR)

-include $(LIB_DEPS)
