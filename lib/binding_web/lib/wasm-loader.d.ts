// Type definitions for the freestanding WASM loader
// Re-export types from the existing web-tree-sitter to maintain compatibility

export { type MainModule } from './web-tree-sitter';

declare function createModule(options?: Partial<EmscriptenModule>): Promise<import('./web-tree-sitter').MainModule>;
export default createModule;