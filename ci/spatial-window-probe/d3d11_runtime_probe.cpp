#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>

#include <d3d11.h>
#include <dxgi.h>
#include <wrl/client.h>

#include <array>
#include <cmath>
#include <cstdint>
#include <iostream>

namespace {
using Microsoft::WRL::ComPtr;

bool close_to(std::uint8_t actual, std::uint8_t expected) {
    return std::abs(static_cast<int>(actual) - static_cast<int>(expected)) <= 1;
}

int fail(const char* operation, HRESULT result) {
    std::cerr << operation << " failed with HRESULT 0x" << std::hex
              << static_cast<unsigned long>(result) << '\n';
    return 1;
}
}  // namespace

int main() {
    constexpr D3D_FEATURE_LEVEL requested_levels[]{
        D3D_FEATURE_LEVEL_11_1,
        D3D_FEATURE_LEVEL_11_0,
    };

    ComPtr<ID3D11Device> device;
    ComPtr<ID3D11DeviceContext> context;
    D3D_FEATURE_LEVEL selected_level{};
    HRESULT result = D3D11CreateDevice(
        nullptr,
        D3D_DRIVER_TYPE_WARP,
        nullptr,
        D3D11_CREATE_DEVICE_BGRA_SUPPORT,
        requested_levels,
        static_cast<UINT>(std::size(requested_levels)),
        D3D11_SDK_VERSION,
        device.GetAddressOf(),
        &selected_level,
        context.GetAddressOf());

    if (result == E_INVALIDARG) {
        result = D3D11CreateDevice(
            nullptr,
            D3D_DRIVER_TYPE_WARP,
            nullptr,
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            &requested_levels[1],
            1U,
            D3D11_SDK_VERSION,
            device.ReleaseAndGetAddressOf(),
            &selected_level,
            context.ReleaseAndGetAddressOf());
    }
    if (FAILED(result)) {
        return fail("D3D11CreateDevice(WARP)", result);
    }

    D3D11_TEXTURE2D_DESC render_desc{};
    render_desc.Width = 16U;
    render_desc.Height = 16U;
    render_desc.MipLevels = 1U;
    render_desc.ArraySize = 1U;
    render_desc.Format = DXGI_FORMAT_B8G8R8A8_UNORM;
    render_desc.SampleDesc = {1U, 0U};
    render_desc.Usage = D3D11_USAGE_DEFAULT;
    render_desc.BindFlags = D3D11_BIND_RENDER_TARGET;

    ComPtr<ID3D11Texture2D> render_texture;
    result = device->CreateTexture2D(&render_desc, nullptr, render_texture.GetAddressOf());
    if (FAILED(result)) {
        return fail("CreateTexture2D(render)", result);
    }

    ComPtr<ID3D11RenderTargetView> render_target;
    result = device->CreateRenderTargetView(render_texture.Get(), nullptr, render_target.GetAddressOf());
    if (FAILED(result)) {
        return fail("CreateRenderTargetView", result);
    }

    constexpr std::array<float, 4> clear_rgba{0.25F, 0.5F, 0.75F, 1.0F};
    context->ClearRenderTargetView(render_target.Get(), clear_rgba.data());

    D3D11_TEXTURE2D_DESC staging_desc = render_desc;
    staging_desc.Usage = D3D11_USAGE_STAGING;
    staging_desc.BindFlags = 0U;
    staging_desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ;

    ComPtr<ID3D11Texture2D> staging_texture;
    result = device->CreateTexture2D(&staging_desc, nullptr, staging_texture.GetAddressOf());
    if (FAILED(result)) {
        return fail("CreateTexture2D(staging)", result);
    }

    context->CopyResource(staging_texture.Get(), render_texture.Get());
    D3D11_MAPPED_SUBRESOURCE mapped{};
    result = context->Map(staging_texture.Get(), 0U, D3D11_MAP_READ, 0U, &mapped);
    if (FAILED(result)) {
        return fail("Map(staging)", result);
    }

    const auto* mapped_bgra = static_cast<const std::uint8_t*>(mapped.pData);
    const std::array<std::uint8_t, 4> bgra{
        mapped_bgra[0], mapped_bgra[1], mapped_bgra[2], mapped_bgra[3]};
    context->Unmap(staging_texture.Get(), 0U);
    const bool pixel_matches = close_to(bgra[0], 191U) && close_to(bgra[1], 128U) &&
                               close_to(bgra[2], 64U) && close_to(bgra[3], 255U);
    if (!pixel_matches) {
        std::cerr << "Unexpected BGRA pixel: " << static_cast<unsigned>(bgra[0]) << ','
                  << static_cast<unsigned>(bgra[1]) << ',' << static_cast<unsigned>(bgra[2])
                  << ',' << static_cast<unsigned>(bgra[3]) << '\n';
        return 1;
    }

    ComPtr<IDXGIDevice> dxgi_device;
    result = device.As(&dxgi_device);
    if (FAILED(result)) {
        return fail("QueryInterface(IDXGIDevice)", result);
    }
    ComPtr<IDXGIAdapter> adapter;
    result = dxgi_device->GetAdapter(adapter.GetAddressOf());
    if (FAILED(result)) {
        return fail("IDXGIDevice::GetAdapter", result);
    }
    DXGI_ADAPTER_DESC adapter_desc{};
    result = adapter->GetDesc(&adapter_desc);
    if (FAILED(result)) {
        return fail("IDXGIAdapter::GetDesc", result);
    }

    std::wcout << L"D3D11 WARP runtime probe passed on adapter: " << adapter_desc.Description
               << L"; feature level 0x" << std::hex << static_cast<unsigned>(selected_level) << L'\n';
    return 0;
}
