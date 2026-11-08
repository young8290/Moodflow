#include "PhysicalKeyRelease.h"
#ifdef NDEBUG
#undef NDEBUG
#endif
#include <cassert>

int main() {
    using moodflow::keys::PhysicalKeyRelease;
    constexpr std::uint32_t key = 13;
    PhysicalKeyRelease release;
    assert(!release.Up(key, true).consume);
    release.Down(key, 3);
    release.Down(key, 0); // Auto-repeat after modifiers change.
    auto first = release.Up(key, true);
    assert(first.consume && first.forward && first.modifiers == 3);
    auto second = release.Up(key, false);
    assert(second.consume && !second.forward);
    assert(!release.Up(key, false).consume);

    // Hosts that skip the test callback still forward once.
    release.Down(key, 2);
    first = release.Up(key, false);
    assert(first.consume && first.forward && first.modifiers == 2);
    assert(!release.Up(key, false).consume);

    // Hosts that skip the actual callback must not carry its consume fact to a new press.
    release.Down(key, 1);
    assert(release.Up(key, true).forward);
    release.Down(key, 2);
    first = release.Up(key, false);
    assert(first.forward && first.modifiers == 2);

    // Two hotkeys may overlap without losing either test/actual pair.
    release.Down(key, 1);
    release.Down(key + 1, 2);
    assert(release.Up(key, true).forward);
    assert(release.Up(key + 1, true).forward);
    assert(release.Up(key, false).consume);
    assert(release.Up(key + 1, false).consume);
}
