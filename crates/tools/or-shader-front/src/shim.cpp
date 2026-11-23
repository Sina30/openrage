// The one place C++ meets Rust: a DXBC blob in, a SPIR-V module or a message out, once per shader.

#include <cstdint>
#include <cstdlib>
#include <cstring>
#include <string>
#include <utility>
#include <vector>

#include "../vendor/dxbc-spirv/dxbc/dxbc_container.h"
#include "../vendor/dxbc-spirv/dxbc/dxbc_converter.h"
#include "../vendor/dxbc-spirv/ir/ir.h"
#include "../vendor/dxbc-spirv/ir/ir_builder.h"
#include "../vendor/dxbc-spirv/ir/ir_legalize.h"
#include "../vendor/dxbc-spirv/spirv/spirv_builder.h"
#include "../vendor/dxbc-spirv/spirv/spirv_mapping.h"
#include "../vendor/dxbc-spirv/util/util_byte_stream.h"
#include "../vendor/dxbc-spirv/util/util_log.h"

using namespace dxbc_spv;

extern "C" {

/// `BindingLayout` in `src/layout.rs`: the same twelve words, in the same order.
struct OrFrontWindow {
  uint32_t base;
  uint32_t len;
};

struct OrFrontLayout {
  OrFrontWindow cbv;
  OrFrontWindow sampler;
  OrFrontWindow srv;
  OrFrontWindow uav;
  OrFrontWindow uav_counter;
  uint32_t set_base;
  uint32_t set_stride;
};

}  // extern "C"

namespace {

/// Keeps the translator's warnings and errors for the caller instead of printing them.
class CapturedLog : public util::Logger {
public:
  void message(util::LogLevel severity, const char* text) override {
    if (severity >= util::LogLevel::eWarn) {
      m_text += text;
      m_text += '\n';
    }
  }

  util::LogLevel getMinimumSeverity() override { return util::LogLevel::eWarn; }

  std::string m_text;
};

/// Places each register at `window.base + register` of its class in the set its register space and stage name, and notes a register its class has no room for.
class LayoutMapping : public spirv::ResourceMapping {
public:
  explicit LayoutMapping(const OrFrontLayout& layout) : m_layout(layout) {}

  spirv::DescriptorBinding mapDescriptor(ir::ScalarType type, uint32_t regSpace,
                                         uint32_t regIndex) override {
    const OrFrontWindow* window = nullptr;
    const char* name = nullptr;
    switch (type) {
      case ir::ScalarType::eCbv:        window = &m_layout.cbv;         name = "constant buffer b"; break;
      case ir::ScalarType::eSampler:    window = &m_layout.sampler;     name = "sampler s"; break;
      case ir::ScalarType::eSrv:        window = &m_layout.srv;         name = "resource t"; break;
      case ir::ScalarType::eUav:        window = &m_layout.uav;         name = "unordered access view u"; break;
      case ir::ScalarType::eUavCounter: window = &m_layout.uav_counter; name = "UAV counter u"; break;
      default:
        note("a descriptor class the layout does not place");
        return {};
    }
    if (regIndex >= window->len) {
      note(std::string(name) + std::to_string(regIndex) + " does not fit the layout's " +
           std::to_string(window->len) + " registers for its class");
      return {};
    }
    spirv::DescriptorBinding binding = {};
    binding.set = m_layout.set_base + m_layout.set_stride * regSpace;
    binding.binding = window->base + regIndex;
    return binding;
  }

  uint32_t mapPushData(ir::ShaderStageMask stages) override { return m_push.mapPushData(stages); }

  const std::string& error() const { return m_error; }

private:
  void note(const std::string& text) {
    if (m_error.empty())
      m_error = text;
  }

  OrFrontLayout m_layout;
  spirv::BasicResourceMapping m_push;
  std::string m_error;
};

char* copy_text(const std::string& text) {
  char* out = static_cast<char*>(std::malloc(text.size() + 1));
  if (out)
    std::memcpy(out, text.c_str(), text.size() + 1);
  return out;
}

}  // namespace

extern "C" {

struct OrFrontResult {
  uint8_t* spirv;
  size_t spirv_len;
  char* message;
};

/// 0 on success. On failure `message` says why, or is null when nothing was logged.
int or_front_dxbc_to_spirv(const uint8_t* data, size_t size, const OrFrontLayout* layout,
                           OrFrontResult* out) {
  out->spirv = nullptr;
  out->spirv_len = 0;
  out->message = nullptr;

  CapturedLog log;
  std::vector<char> binary;
  bool ok = false;

  dxbc::Container container(util::ByteReader(data, size));
  if (container) {
    dxbc::Converter::Options convert = {};
    convert.includeDebugNames = false;
    dxbc::Converter converter(std::move(container), convert);

    ir::Builder builder;
    if (converter.convertShader(builder)) {
      // Options chosen for naga: constant buffers as vector arrays, structured buffers as nested
      // arrays, no rounding or denorm modes asked of the device, and float-to-integer conversion as a
      // native instruction, since a call to a helper function trips naga's Metal writer.
      ir::CompileOptions compile = {};
      compile.cseOptions.relocateDescriptorLoad = true;
      compile.descriptorIndexing.optimizeDescriptorIndexing = true;
      compile.resourceOptions.structuredCbv = false;
      compile.resourceOptions.structuredSrvUav = false;
      compile.arithmeticOptions.lowerConvertFtoI = false;
      ir::legalizeIr(builder, compile);

      LayoutMapping mapping(*layout);
      spirv::SpirvBuilder::Options spirv = {};
      spirv.includeDebugNames = false;
      spirv.floatControls2 = false;

      spirv::SpirvBuilder spirvBuilder(builder, mapping, spirv);
      spirvBuilder.buildSpirvBinary();

      size_t length = 0;
      spirvBuilder.getSpirvBinary(length, nullptr);
      binary.resize(length);
      spirvBuilder.getSpirvBinary(length, binary.data());
      ok = length != 0;
      if (!mapping.error().empty()) {
        log.m_text += mapping.error() + "\n";
        ok = false;
      }
    }
  } else {
    log.m_text = "not a DXBC container\n";
  }

  if (!ok) {
    if (!log.m_text.empty())
      out->message = copy_text(log.m_text);
    return 1;
  }
  out->spirv = static_cast<uint8_t*>(std::malloc(binary.size()));
  if (!out->spirv)
    return 1;
  std::memcpy(out->spirv, binary.data(), binary.size());
  out->spirv_len = binary.size();
  if (!log.m_text.empty())
    out->message = copy_text(log.m_text);
  return 0;
}

void or_front_free(OrFrontResult* result) {
  std::free(result->spirv);
  std::free(result->message);
  result->spirv = nullptr;
  result->message = nullptr;
  result->spirv_len = 0;
}

}  // extern "C"
