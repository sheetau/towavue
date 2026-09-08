#include <iostream>
#include <vector>

extern "C" __declspec(dllimport) int smoke_value(void);

int main() {
    const std::vector<int> values{smoke_value(), 8};
    if (values[0] + values[1] != 50) return 1;
    std::cout << "Native GNU C DLL and C++ executable passed.\n";
}
