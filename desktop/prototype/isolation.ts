// Fixtures in separate tabs can use the normal preference APIs independently.
Object.defineProperty(window, 'localStorage', {value: window.sessionStorage, configurable: true});
