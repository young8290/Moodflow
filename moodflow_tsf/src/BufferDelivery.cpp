#include "TextService.h"
#include "IPCClient.h"

// All checks and all text mutations run on the owning TSF thread. Holding the exact context
// prevents an asynchronous edit session from silently resolving the new focused field.
class CBufferDeliveryEditSession final : public ITfEditSession {
public:
    CBufferDeliveryEditSession(CTextService* service, ITfContext* context,
                              const moodflow::buffer::Delivery& delivery)
        : _service(service), _context(context), _delivery(delivery) {
        _service->AddRef();
        _context->AddRef();
    }
    ~CBufferDeliveryEditSession() {
        // A cancelled edit session never entered the write API.
        Complete(moodflow::buffer::Outcome::Rejected);
        _context->Release();
        _service->Release();
    }
    STDMETHODIMP QueryInterface(REFIID iid, void** out) override {
        if (!out) return E_INVALIDARG;
        *out = nullptr;
        if (IsEqualIID(iid, IID_IUnknown) || IsEqualIID(iid, IID_ITfEditSession)) {
            *out = static_cast<ITfEditSession*>(this);
            AddRef();
            return S_OK;
        }
        return E_NOINTERFACE;
    }
    STDMETHODIMP_(ULONG) AddRef() override { return InterlockedIncrement(&_refs); }
    STDMETHODIMP_(ULONG) Release() override {
        const auto refs = InterlockedDecrement(&_refs);
        if (!refs) delete this;
        return refs;
    }
    STDMETHODIMP DoEditSession(TfEditCookie cookie) override {
        using moodflow::buffer::Outcome;
        if (_completed || !_service->IsBufferTargetCurrent(_delivery, _context)) {
            Complete(Outcome::Rejected);
            return E_ABORT;
        }
        ITfInsertAtSelection* insert = nullptr;
        if (FAILED(_context->QueryInterface(IID_ITfInsertAtSelection, reinterpret_cast<void**>(&insert)))
            || !insert) {
            Complete(Outcome::Rejected);
            return E_NOINTERFACE;
        }
        // The service checks the exact request, current binding, privacy and deadline in memory.
        if (!_service->_pIPCClient || !_service->_pIPCClient->AuthorizeBufferDelivery(_delivery)
            || !_service->IsBufferTargetCurrent(_delivery, _context)) {
            insert->Release();
            Complete(Outcome::Rejected);
            return E_ABORT;
        }
        ITfRange* range = nullptr;
        const HRESULT hr = insert->InsertTextAtSelection(cookie, 0, _delivery.text.c_str(),
                                                         static_cast<LONG>(_delivery.text.size()), &range);
        insert->Release();
        // A host can mutate before returning an error. Never retry that case automatically.
        Complete(SUCCEEDED(hr) ? Outcome::Accepted : Outcome::Unknown);
        if (SUCCEEDED(hr) && range) {
            if (SUCCEEDED(range->Collapse(cookie, TF_ANCHOR_END))) {
                TF_SELECTION selection = {};
                selection.range = range;
                selection.style.ase = TF_AE_NONE;
                _context->SetSelection(cookie, 1, &selection);
            }
        }
        if (range) range->Release();
        return hr;
    }
private:
    void Complete(moodflow::buffer::Outcome outcome) {
        if (_completed) return;
        _completed = true;
        if (_service->_pIPCClient) _service->_pIPCClient->SendBufferReceipt(_delivery, outcome);
    }
    LONG _refs = 1;
    CTextService* _service;
    ITfContext* _context;
    moodflow::buffer::Delivery _delivery;
    bool _completed = false;
};

bool CTextService::IsBufferTargetCurrent(const moodflow::buffer::Delivery& delivery, ITfContext* expected)
{
    if (!_pIPCClient || !_pThreadMgr) return false;
    constexpr UINT64 kPrivateScopes = (1ULL << 31) | (1ULL << 61) | (1ULL << 63);
    if (!moodflow::buffer::IsCurrent(delivery, _pIPCClient->GetClientToken(), _focusSessionId,
        GetTickCount64(), _hasFocus && _hasThreadFocus && _hasTextInputContext,
        _bKeyboardDisabled || _focusIsPassword || (_focusInputScopeMask & kPrivateScopes),
        _pComposition != nullptr || !_pendingCommitPrefix.empty())) return false;
    ITfDocumentMgr* document = nullptr;
    ITfContext* current = nullptr;
    if (SUCCEEDED(_pThreadMgr->GetFocus(&document)) && document) {
        document->GetTop(&current);
        document->Release();
    }
    const bool same = current && (!expected || current == expected);
    TF_STATUS status = {};
    const bool writable = same && SUCCEEDED(current->GetStatus(&status))
        && !(status.dwDynamicFlags & TF_SD_READONLY);
    if (current) current->Release();
    return writable;
}

void CTextService::DeliverBuffer(const moodflow::buffer::Delivery& delivery)
{
    if (!IsBufferTargetCurrent(delivery, nullptr)) {
        if (_pIPCClient) _pIPCClient->SendBufferReceipt(delivery, moodflow::buffer::Outcome::Rejected);
        return;
    }
    ITfDocumentMgr* document = nullptr;
    ITfContext* context = nullptr;
    if (SUCCEEDED(_pThreadMgr->GetFocus(&document)) && document) {
        document->GetTop(&context);
        document->Release();
    }
    if (!context) {
        _pIPCClient->SendBufferReceipt(delivery, moodflow::buffer::Outcome::Rejected);
        return;
    }
    auto* session = new CBufferDeliveryEditSession(this, context, delivery);
    HRESULT result = E_FAIL;
    context->RequestEditSession(_tfClientId, session, TF_ES_ASYNCDONTCARE | TF_ES_READWRITE, &result);
    session->Release();
    context->Release();
}
