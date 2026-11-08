#include "BufferDelivery.h"
#ifdef NDEBUG
#undef NDEBUG
#endif
#include <cassert>

int main() {
    using namespace moodflow::buffer;
    Delivery request;
    request.id = 10;
    request.client = 0x0100000002;
    request.revision = 5;
    request.receivedAt = 100;
    assert(IsCurrent(request, request.client, 5, 101, true, false, false));
    assert(!IsCurrent(request, request.client, 6, 101, true, false, false));
    assert(!IsCurrent(request, request.client + 1, 5, 101, true, false, false));
    assert(!IsCurrent(request, request.client, 5, 101, false, false, false));
    assert(!IsCurrent(request, request.client, 5, 101, true, true, false));
    assert(!IsCurrent(request, request.client, 5, 101, true, false, true));
    assert(!IsCurrent(request, request.client, 5, 5100, true, false, false));
    assert(!IsCurrent(request, request.client, 5, 99, true, false, false));
    auto body = Identity(request);
    Append(body, 3, 4);
    body.insert(body.end(), {'a', 'b', 'c'});
    auto frame = Envelope(kDeliver, body);
    const std::uint8_t* decoded = nullptr;
    std::size_t size = 0;
    assert(DecodeEnvelope(frame.data(), frame.size(), kDeliver, decoded, size));
    const char* text = nullptr;
    std::size_t bytes = 0;
    Delivery parsed;
    assert(DecodeDeliveryHead(decoded, size, parsed, text, bytes));
    assert(parsed.id == request.id && parsed.client == request.client && parsed.revision == 5);
    assert(std::string(text, bytes) == "abc");
    for (std::size_t i = 0; i < frame.size(); ++i)
        assert(!DecodeEnvelope(frame.data(), i, kDeliver, decoded, size));
    body.back() = 0;
    assert(!DecodeDeliveryHead(body.data(), body.size(), parsed, text, bytes));
    return 0;
}
