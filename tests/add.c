int _start() {
    register int a asm ("a1") = 1 ;
    register int b asm ("a2") = 2;
    asm volatile("" : "+r"(a));
    asm volatile("" : "+r"(b));
    return a - b;
}