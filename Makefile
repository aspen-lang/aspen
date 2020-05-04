PROFILE ?= release

build: aspen-cli/target/$(PROFILE)/aspen

.PHONY: aspen-cli/target/$(PROFILE)/aspen
aspen-cli/target/$(PROFILE)/aspen: aspen-cli/target/$(PROFILE)/libaspen_runtime.a
ifeq ($(PROFILE), release)
	( cd aspen-cli; cargo build --release )
else
	( cd aspen-cli; cargo build )
endif

aspen-cli/target/$(PROFILE)/libaspen_runtime.a: aspen-runtime/target/$(PROFILE)/libaspen_runtime.a
	cp aspen-runtime/target/$(PROFILE)/libaspen_runtime.a aspen-cli/target/$(PROFILE)/libaspen_runtime.a

.PHONY: aspen-runtime/target/$(PROFILE)/libaspen_runtime.a
aspen-runtime/target/$(PROFILE)/libaspen_runtime.a:
ifeq ($(PROFILE), release)
	( cd aspen-runtime; cargo build --release )
else
	( cd aspen-runtime; cargo build )
endif

.PHONY: fmt
fmt:
	( cd aspen; cargo fmt )
	( cd aspen-cli; cargo fmt )
	( cd aspen-runtime; cargo fmt )

.PHONY: test
test:
	( cd aspen; cargo test --lib )
	( cd aspen-cli; cargo test )
	( cd aspen-runtime; cargo test --lib )
