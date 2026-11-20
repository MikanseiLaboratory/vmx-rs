// Thin helpers around libvmx for the comparison harness.
//
// VMX_SetThreads and VMX_Destroy are already exported. The AVX2 bit is a
// field on the public VMX_INSTANCE struct with no setter; the x86-64 run
// forces it off to time the 128-bit path. On the ARM build that field is
// ignored (the 256-bit functions are not compiled).

#include "vmxcodec.h"

extern "C" {

void vmxref_set_avx2(VMX_INSTANCE *instance, int enable) {
    if (instance) {
        instance->avx2 = enable ? 1 : 0;
    }
}

int vmxref_get_avx2(VMX_INSTANCE *instance) {
    return (instance && instance->avx2) ? 1 : 0;
}

void vmxref_set_threads(VMX_INSTANCE *instance, int threads) {
    VMX_SetThreads(instance, threads);
}

void vmxref_destroy(VMX_INSTANCE *instance) { VMX_Destroy(instance); }

}
