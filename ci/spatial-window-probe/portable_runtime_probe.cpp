#include <array>
#include <cstdint>
#include <iostream>
#include <numeric>

int main() {
    constexpr std::array<std::uint32_t, 5> values{1U, 2U, 3U, 5U, 8U};
    const auto sum = std::accumulate(values.begin(), values.end(), std::uint32_t{0});
    if (sum != 19U) {
        std::cerr << "portable runtime probe failed\n";
        return 1;
    }
    std::cout << "portable runtime probe passed\n";
    return 0;
}
