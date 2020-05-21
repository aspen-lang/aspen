PROFILE ?= release

build: aspen-cli/target/$(PROFILE)/aspen

.PHONY: aspen-cli/target/$(PROFILE)/aspen
aspen-cli/target/$(PROFILE)/aspen: aspen-runtime/target/$(PROFILE)/libaspen_runtime.a
	mkdir -p aspen-cli/target/$(PROFILE)/deps
	cp aspen-runtime/target/$(PROFILE)/libaspen_runtime.a aspen-cli/target/$(PROFILE)/deps/libaspen_runtime.a
	cp aspen-runtime/target/$(PROFILE)/libaspen_runtime.a aspen-cli/target/$(PROFILE)/libaspen_runtime.a
ifeq ($(PROFILE), release)
	( cd aspen-cli; cargo build --release )
else
	( cd aspen-cli; cargo build )
endif

.PHONY: aspen-runtime/target/$(PROFILE)/libaspen_runtime.a
aspen-runtime/target/$(PROFILE)/libaspen_runtime.a:
ifeq ($(PROFILE), release)
	( cd aspen-runtime; cargo build --features=standalone --release )
else
	( cd aspen-runtime; cargo build --features=standalone )
endif

.PHONY: fmt
fmt:
	( cd aspen; cargo fmt )
	( cd aspen-cli; cargo fmt )
	( cd aspen-runtime; cargo fmt )

.PHONY: test
test: aspen-runtime/target/$(PROFILE)/libaspen_runtime.a
	mkdir -p aspen-cli/target/$(PROFILE)/deps
	cp aspen-runtime/target/$(PROFILE)/libaspen_runtime.a aspen-cli/target/$(PROFILE)/deps/libaspen_runtime.a
	cp aspen-runtime/target/$(PROFILE)/libaspen_runtime.a aspen-cli/target/$(PROFILE)/libaspen_runtime.a
	( cd aspen; cargo test --lib )
	( cd aspen-cli; cargo test )
	( cd aspen-runtime; cargo test --lib )

.PHONY: download-platform-schema
download-platform-schema:
	echo '# GENERATED FILE – DO NOT EDIT' > aspen-cli/src/platform/schema.graphql
	npx get-graphql-schema https://platform.aspen-lang.com >> aspen-cli/src/platform/schema.graphql
