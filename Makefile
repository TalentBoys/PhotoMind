.PHONY: build build-web build-rust dev run clean

# === 生产模式 ===

# 完整构建（前端 + 后端 release）
build: build-web build-rust

build-rust:
	cargo build --release

build-web:
	cd web && npm run build

# 构建并启动
run: build
	./target/release/photomind

# === 开发模式 ===

# 前后端同时启动（后端 debug + 前端 HMR）
dev:
	cd web && npm run dev &
	cargo run

# === 清理 ===

clean:
	cargo clean
	rm -rf web/dist
