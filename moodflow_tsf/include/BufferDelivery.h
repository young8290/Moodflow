#pragma once

#include <cstdint>
#include <cstring>
#include <string>
#include <vector>

// Mirrored by moodflow-ipc/src/buffer.rs. No OS dependencies: wire checks are testable.
namespace moodflow::buffer {
constexpr const char* kDeliver = "buffer.deliver";
constexpr const char* kReceipt = "buffer.receipt";
constexpr const char* kAuthorize = "buffer.authorize";
constexpr const char* kMode = "buffer.mode";
constexpr std::size_t kMaxUtf16 = 16384;
constexpr std::uint64_t kTimeoutMs = 5000;
enum class Outcome : std::uint8_t { Accepted = 0, Rejected = 1, Unknown = 2 };

inline void Append(std::vector<std::uint8_t>& out, std::uint64_t value, unsigned count) {
    for (unsigned i = 0; i < count; ++i) out.push_back(static_cast<std::uint8_t>(value >> (8 * i)));
}
inline std::uint64_t Read(const std::uint8_t* data, unsigned count) {
    std::uint64_t value = 0;
    for (unsigned i = 0; i < count; ++i) value |= std::uint64_t(data[i]) << (8 * i);
    return value;
}

struct Delivery {
    std::uint64_t id = 0;
    std::uint64_t client = 0;
    std::uint64_t revision = 0;
    std::uint64_t receivedAt = 0; // local monotonic tick, not part of the wire
    std::wstring text;
};

inline std::vector<std::uint8_t> Identity(const Delivery& delivery) {
    std::vector<std::uint8_t> body;
    Append(body, delivery.id, 8);
    Append(body, delivery.client, 8);
    Append(body, delivery.revision, 8);
    return body;
}

inline std::vector<std::uint8_t> Envelope(const char* kind, const std::vector<std::uint8_t>& body) {
    std::vector<std::uint8_t> out;
    const auto length = std::strlen(kind);
    Append(out, length, 4);
    out.insert(out.end(), kind, kind + length);
    Append(out, body.size(), 4);
    out.insert(out.end(), body.begin(), body.end());
    return out;
}

inline bool DecodeEnvelope(const std::uint8_t* data, std::size_t size,
                           const char* kind, const std::uint8_t*& body, std::size_t& bodySize) {
    if (size < 8) return false;
    const auto length = static_cast<std::size_t>(Read(data, 4));
    if (length != std::strlen(kind) || length > size - 8 || std::memcmp(data + 4, kind, length) != 0)
        return false;
    bodySize = static_cast<std::size_t>(Read(data + 4 + length, 4));
    if (bodySize != size - 8 - length) return false;
    body = data + 8 + length;
    return true;
}

inline bool DecodeDeliveryHead(const std::uint8_t* body, std::size_t size, Delivery& delivery,
                               const char*& utf8, std::size_t& bytes) {
    if (size < 28) return false;
    bytes = static_cast<std::size_t>(Read(body + 24, 4));
    if (bytes == 0 || bytes > kMaxUtf16 * 3 || bytes != size - 28) return false;
    delivery.id = Read(body, 8);
    delivery.client = Read(body + 8, 8);
    delivery.revision = Read(body + 16, 8);
    utf8 = reinterpret_cast<const char*>(body + 28);
    return delivery.revision != 0 && std::memchr(utf8, 0, bytes) == nullptr;
}

inline bool IsCurrent(const Delivery& delivery, std::uint64_t client, std::uint64_t revision,
                      std::uint64_t now, bool focused, bool privateField, bool composing) {
    return focused && !privateField && !composing && delivery.client == client
        && delivery.revision != 0 && delivery.revision == revision
        && now >= delivery.receivedAt && now - delivery.receivedAt < kTimeoutMs;
}
} // namespace moodflow::buffer
