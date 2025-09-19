/**
 * Minimal WebAssembly loader for Tree-sitter (no Emscripten runtime)
 * 
 * This replaces the heavy Emscripten runtime with a lightweight loader
 * that provides only the essentials for Tree-sitter web bindings.
 */

class TreeSitterWasm {
  constructor() {
    this.instance = null;
    this.memory = null;
    this.exports = null;
    this.heap = null;
  }

  async init(wasmBytes, imports = {}) {
    // Create shared memory (512 pages initial as required by WASM, can grow)
    this.memory = new WebAssembly.Memory({ 
      initial: 512,  // 512 pages = 32MB (as required by our WASM)
      maximum: 2048, // 128MB max
      shared: false 
    });

    // Create function table (30+ functions initial as required by WASM)
    this.table = new WebAssembly.Table({
      initial: 64,   // 64 function slots (more than the required 30)
      maximum: 1024, // Allow growth
      element: 'anyfunc'  // Use 'anyfunc' for broader compatibility
    });

    // Required imports for freestanding WASM
    const wasmImports = {
      env: {
        memory: this.memory,
        __indirect_function_table: this.table,
        
        // Tree-sitter callbacks (will be set by TypeScript code)
        tree_sitter_log_callback: () => {},
        tree_sitter_parse_callback: () => 0,
        tree_sitter_progress_callback: () => {},
        tree_sitter_query_progress_callback: () => {},
        
        // Runtime functions
        abort: () => { throw new Error('WASM abort'); },
        _abort_js: () => { throw new Error('WASM abort'); },
        emscripten_resize_heap: () => false, // Don't allow heap resize
        
        // Global pointers (will be set by WASM)
        __stack_pointer: new WebAssembly.Global({ value: 'i32', mutable: true }, 0),
        __memory_base: new WebAssembly.Global({ value: 'i32', mutable: false }, 0),
        __table_base: new WebAssembly.Global({ value: 'i32', mutable: false }, 0),
        
        ...imports
      },
      
      // WASI stubs (empty implementations)
      wasi_snapshot_preview1: {
        fd_close: () => 0,
        fd_write: () => 0, 
        fd_seek: () => 0,
      },
      
      // GOT (Global Offset Table) stubs
      "GOT.mem": {
        __stack_low: new WebAssembly.Global({ value: 'i32', mutable: true }, 0),
        __stack_high: new WebAssembly.Global({ value: 'i32', mutable: true }, 65536),
        __heap_base: new WebAssembly.Global({ value: 'i32', mutable: true }, 1024),
      }
    };

    const module = await WebAssembly.compile(wasmBytes);
    this.instance = await WebAssembly.instantiate(module, wasmImports);
    this.exports = this.instance.exports;
    
    // Create typed array views of memory
    this.updateHeapViews();
    
    // Call WASM constructors if present
    if (this.exports.__wasm_call_ctors) {
      this.exports.__wasm_call_ctors();
    }

    return this;
  }

  updateHeapViews() {
    const buffer = this.memory.buffer;
    this.heap = {
      HEAP8: new Int8Array(buffer),
      HEAPU8: new Uint8Array(buffer),
      HEAP16: new Int16Array(buffer),
      HEAPU16: new Uint16Array(buffer),
      HEAP32: new Int32Array(buffer),
      HEAPU32: new Uint32Array(buffer),
      HEAPF32: new Float32Array(buffer),
      HEAPF64: new Float64Array(buffer),
    };
  }

  // Memory management
  malloc(size) {
    return this.exports.malloc ? this.exports.malloc(size) : 0;
  }

  free(ptr) {
    if (this.exports.free && ptr) {
      this.exports.free(ptr);
    }
  }

  // String utilities
  stringToUTF8(str, outPtr, maxBytesToWrite) {
    const encoded = new TextEncoder().encode(str + '\0');
    const len = Math.min(encoded.length, maxBytesToWrite - 1);
    this.heap.HEAPU8.set(encoded.subarray(0, len), outPtr);
    this.heap.HEAPU8[outPtr + len] = 0; // null terminator
    return len;
  }

  UTF8ToString(ptr, maxBytesToRead = -1) {
    if (!ptr) return '';
    
    let end = ptr;
    const maxEnd = maxBytesToRead < 0 ? this.heap.HEAPU8.length : ptr + maxBytesToRead;
    
    // Find null terminator
    while (end < maxEnd && this.heap.HEAPU8[end] !== 0) end++;
    
    return new TextDecoder().decode(this.heap.HEAPU8.subarray(ptr, end));
  }

  lengthBytesUTF8(str) {
    return new TextEncoder().encode(str).length;
  }

  // Memory access
  getValue(ptr, type = 'i32') {
    switch (type) {
      case 'i8': return this.heap.HEAP8[ptr];
      case 'i16': return this.heap.HEAP16[ptr >> 1];
      case 'i32': return this.heap.HEAP32[ptr >> 2];
      case 'i64': return this.heap.HEAP32[ptr >> 2]; // Limited to 32-bit for now
      case 'float': return this.heap.HEAPF32[ptr >> 2];
      case 'double': return this.heap.HEAPF64[ptr >> 3];
      default: return this.heap.HEAP32[ptr >> 2];
    }
  }

  setValue(ptr, value, type = 'i32') {
    switch (type) {
      case 'i8': this.heap.HEAP8[ptr] = value; break;
      case 'i16': this.heap.HEAP16[ptr >> 1] = value; break;
      case 'i32': this.heap.HEAP32[ptr >> 2] = value; break;
      case 'float': this.heap.HEAPF32[ptr >> 2] = value; break;
      case 'double': this.heap.HEAPF64[ptr >> 3] = value; break;
      default: this.heap.HEAP32[ptr >> 2] = value; break;
    }
  }

  // Load external WASM module (for language parsers)
  async loadWebAssemblyModule(binary, flags = {}) {
    const module = binary instanceof WebAssembly.Module ? 
      binary : await WebAssembly.compile(binary);
    
    const instance = await WebAssembly.instantiate(module, {
      env: {
        memory: this.memory,
        ...flags
      }
    });

    // Return exported functions
    const exports = {};
    for (const [name, fn] of Object.entries(instance.exports)) {
      if (typeof fn === 'function') {
        exports[name] = () => fn;
      }
    }
    return exports;
  }
}

// Factory function matching Emscripten API
export default async function createModule(options = {}) {
  const loader = new TreeSitterWasm();
  
  // Load WASM file
  let wasmBytes;
  if (options.wasmBinary) {
    wasmBytes = options.wasmBinary;
  } else {
    // Try to load from same directory
    try {
      if (typeof fetch !== 'undefined') {
        // Browser environment
        const wasmPath = new URL('./web-tree-sitter.wasm', import.meta.url);
        const response = await fetch(wasmPath);
        wasmBytes = await response.arrayBuffer();
      } else {
        // Node.js environment - read file directly
        const fs = await import('fs');
        const path = await import('path');
        const { fileURLToPath } = await import('url');
        
        const __filename = fileURLToPath(import.meta.url);
        const __dirname = path.dirname(__filename);
        const wasmPath = path.join(path.dirname(__dirname), 'web-tree-sitter.wasm');
        
        wasmBytes = fs.readFileSync(wasmPath);
      }
    } catch (error) {
      throw new Error(`Failed to load WASM file: ${error.message}`);
    }
  }

  await loader.init(new Uint8Array(wasmBytes), options.imports);
  
  // Return object compatible with existing TypeScript interfaces
  return {
    ...loader.exports,
    ...loader.heap,
    
    // Runtime methods
    stringToUTF8: loader.stringToUTF8.bind(loader),
    UTF8ToString: loader.UTF8ToString.bind(loader),
    lengthBytesUTF8: loader.lengthBytesUTF8.bind(loader),
    getValue: loader.getValue.bind(loader),
    setValue: loader.setValue.bind(loader),
    loadWebAssemblyModule: loader.loadWebAssemblyModule.bind(loader),
    
    // Callback placeholders (set by TypeScript code)
    currentParseCallback: null,
    currentLogCallback: null,
    currentProgressCallback: null,
    currentQueryProgressCallback: null,
  };
}
