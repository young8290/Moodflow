#pragma once
#include <cstdint>
#include <map>
#include <set>

namespace moodflow::keys {
struct KeyRelease {
    bool consume = false;
    bool forward = false;
    std::uint32_t modifiers = 0;
};

// Some TSF hosts call only OnTestKeyUp; others call both test and actual callbacks.
// Forward exactly once, while reporting the same consumed result to both callbacks.
class PhysicalKeyRelease {
public:
    void Down(std::uint32_t key, std::uint32_t modifiers) {
        tested_.erase(key);
        down_.emplace(key, modifiers); // Repeats keep the original modifier snapshot.
    }

    KeyRelease Up(std::uint32_t key, bool testCallback) {
        if (!testCallback && tested_.erase(key) != 0) return {true, false, 0};
        auto found = down_.find(key);
        if (found == down_.end()) return {};
        const auto modifiers = found->second;
        down_.erase(found);
        if (testCallback) tested_.insert(key);
        return {true, true, modifiers};
    }

private:
    std::map<std::uint32_t, std::uint32_t> down_;
    std::set<std::uint32_t> tested_;
};
}
