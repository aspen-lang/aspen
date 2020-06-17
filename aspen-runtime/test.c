#include <libaspenrt.h>
#include <stdio.h>
#include <stdint.h>
#include <time.h>
#include <unistd.h>

void Echo(const rt_t *rt, const object_ptr *self, void *state, object_ptr reply_to, object_ptr msg) {
  AspenTell(&reply_to, msg);
  AspenDrop(reply_to);
}

typedef struct {
  object_ptr a;
  object_ptr b;
} Frame;

void DropFrame(const rt_t *rt, void *frame) {
  Frame *f = frame;
  AspenDrop(f->a);
  AspenDrop(f->b);
  puts("Dropped frame");
}

void B(const rt_t *rt, const object_ptr *self, void *state, void *continuationFrame, object_ptr reply_to, object_ptr msg) {
  Frame *frame = continuationFrame;
  AspenPrint(&frame->a);
  AspenPrint(&frame->b);
  AspenPrint(&msg);
  AspenPrint(&reply_to);
  AspenDrop(msg);
  AspenDrop(reply_to);
}

void Print(const rt_t *rt, const object_ptr *self, void *state, object_ptr reply_to, object_ptr msg) {
  AspenPrint(&msg);
  AspenDrop(reply_to);
  AspenDrop(msg);
}

void A(const rt_t *rt, const object_ptr *self, void *state, object_ptr reply_to, object_ptr msg) {
  object_ptr echo = AspenNewStatelessActor(rt, &Echo);

  Frame *frame;
  object_ptr continuation = AspenContinue(rt, self, sizeof(int), (void *)&frame, &B, &DropFrame);
  frame->a = AspenNewInt(123);
  frame->b = AspenNewInt(234);

  AspenAsk(&echo, continuation, msg);
  AspenDrop(reply_to);
}

void start(const rt_t *rt) {
  object_ptr a = AspenNewStatelessActor(rt, &A);

  AspenTell(&a, AspenNewAtom("init!"));

  AspenDrop(a);
}

int main() {
  AspenStartRuntime(&start);
}
