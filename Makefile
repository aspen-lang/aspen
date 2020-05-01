PROFILE ?= release

.PHONY: aspen-cli/target/$(PROFILE)/aspen
aspen-cli/target/$(PROFILE)/aspen: aspen-cli/target/$(PROFILE)/libaspen.a
ifeq ($(PROFILE), release)
	( cd aspen-cli; cargo build --release )
else
	( cd aspen-cli; cargo build )
endif

aspen-cli/target/$(PROFILE)/libaspen.a: aspen-runtime/target/$(PROFILE)/libaspen.a
	cp aspen-runtime/target/$(PROFILE)/libaspen.a aspen-cli/target/$(PROFILE)/libaspen.a

.PHONY: aspen-runtime/target/$(PROFILE)/libaspen.a
aspen-runtime/target/$(PROFILE)/libaspen.a:
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
