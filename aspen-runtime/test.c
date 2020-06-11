#include <libaspenrt.h>
#include <stdio.h>
#include <stdint.h>

typedef struct {
  unsigned long long count;
} CounterState;

void CounterInit(const rt_t *rt, const object_ptr *self, void *state_ptr) {
  CounterState *state = state_ptr;
  state->count = 0;
}

void CounterRecv(const rt_t *rt, const object_ptr *self, void *state_ptr, object_ptr msg) {
  CounterState *state = state_ptr;
  state->count++;
  printf(".");
  AspenSend(self, msg);
}

object_ptr CounterNew(const rt_t *rt) {
  return AspenNewActor(rt, sizeof(CounterState), &CounterInit, &CounterRecv);
}

void start(const rt_t *rt) {
  for (int i = 0; i < 1000000; i++) {
    object_ptr counter = CounterNew(rt);
    AspenSend(&counter, AspenNewAtom("start!"));
    AspenDrop(counter);
  }
}

int main() {
  AspenStartRuntime(&start);
}
