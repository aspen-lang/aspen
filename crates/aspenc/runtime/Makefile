ERL ?= erl
ERLC ?= erlc
CC ?= cc
ERL_ROOT ?= $(shell $(ERL) +S 1:1 -noshell -eval 'io:put_chars(code:root_dir()), halt().')
CFLAGS ?= -O2 -Wall -Wextra -Werror
UNAME := $(shell uname -s)
ifeq ($(UNAME),Darwin)
SHARED_FLAGS := -dynamiclib -undefined dynamic_lookup
else
SHARED_FLAGS := -shared
endif

.PHONY: all clean
all: aspen_runtime.beam aspen_syscall.beam aspen_syscall_nif.so

%.beam: %.erl
	$(ERLC) -Werror $<

aspen_syscall_nif.so: aspen_syscall_nif.c
	$(CC) $(CFLAGS) -fPIC $(SHARED_FLAGS) -I"$(ERL_ROOT)/usr/include" -o $@ $<

clean:
	rm -f aspen_runtime.beam aspen_syscall.beam aspen_syscall_nif.so
