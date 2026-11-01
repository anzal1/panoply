/**
 * Passport and visa photos: the desktop pipeline (rules, face geometry, warp, exposure, background, checks) with the model runs
 * (YuNet faces, an optional IS-Net cut-out) done by the browser between steps.
 */
export class Passport {
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        PassportFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_passport_free(ptr, 0);
    }
    /**
     * @returns {Float32Array | undefined}
     */
    face_next() {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16) >>> 0;
            wasm.passport_face_next(retptr, this.__wbg_ptr);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            let v1;
            if (r0 !== 0) {
                v1 = getArrayF32FromWasm0(r0, r1).slice();
                wasm.__wbindgen_export(r0, r1 * 4, 4);
            }
            return v1;
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    face_pass_done() {
        wasm.passport_face_pass_done(this.__wbg_ptr);
    }
    /**
     * @param {number} stride
     * @param {Float32Array | number[]} cls
     * @param {Float32Array | number[]} obj
     * @param {Float32Array | number[]} bbox
     * @param {Float32Array | number[]} kps
     */
    face_put(stride, cls, obj, bbox, kps) {
        const val5 = +stride;
        let ptr0, len0, ptr1, len1, ptr2, len2, ptr3, len3;
        try {
            ptr0 = passArrayF32ToWasm0(cls, wasm.__wbindgen_export2);
            len0 = WASM_VECTOR_LEN;
            ptr1 = passArrayF32ToWasm0(obj, wasm.__wbindgen_export2);
            len1 = WASM_VECTOR_LEN;
            ptr2 = passArrayF32ToWasm0(bbox, wasm.__wbindgen_export2);
            len2 = WASM_VECTOR_LEN;
            ptr3 = passArrayF32ToWasm0(kps, wasm.__wbindgen_export2);
            len3 = WASM_VECTOR_LEN;
        } catch (e) {
            if (ptr3) { wasm.__wbindgen_export(ptr3, len3 * 4, 4); }
            if (ptr2) { wasm.__wbindgen_export(ptr2, len2 * 4, 4); }
            if (ptr1) { wasm.__wbindgen_export(ptr1, len1 * 4, 4); }
            if (ptr0) { wasm.__wbindgen_export(ptr0, len0 * 4, 4); }
            throw e;
        }
        wasm.passport_face_put(this.__wbg_ptr, val5, ptr0, len0, ptr1, len1, ptr2, len2, ptr3, len3);
    }
    /**
     * The faces found, JSON [{x, y, w, h, confidence}], left to right.
     * @returns {string}
     */
    faces_json() {
        let deferred1_0;
        let deferred1_1;
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16) >>> 0;
            wasm.passport_faces_json(retptr, this.__wbg_ptr);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            deferred1_0 = r0;
            deferred1_1 = r1;
            return getStringFromWasm0(r0, r1);
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
            wasm.__wbindgen_export(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * The photo, flattened on white and brought to at most 4096 px on the long side.
     * @param {Uint8Array | number[]} rgba
     * @param {number} w
     * @param {number} h
     */
    constructor(rgba, w, h) {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16) >>> 0;
            const val2 = +w;
            const val3 = +h;
            const ptr0 = passArray8ToWasm0(rgba, wasm.__wbindgen_export2);
            const len0 = WASM_VECTOR_LEN;
            wasm.passport_new(retptr, ptr0, len0, val2, val3);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            var r2 = getDataViewMemory0().getInt32(retptr + 4 * 2, true);
            if (r2) {
                throw takeFromExternrefTable0(r1);
            }
            this.__wbg_ptr = r0;
            PassportFinalization.register(this, this.__wbg_ptr, this);
            return this;
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
     * Choose the face, cut the person out and work out the crop. `face_index` is 1 based, 0 chooses automatically.
     * `matte` is an optional IS-Net mask at the picture's size (empty uses the desktop's plain backdrop colour cut).
     * @param {string} rule_id
     * @param {number} face_index
     * @param {boolean} fix_tilt
     * @param {boolean} fix_exposure
     * @param {boolean} whiten
     * @param {boolean} color_cutout
     * @param {string} background
     * @param {Uint8Array | number[]} matte
     * @returns {string}
     */
    prepare(rule_id, face_index, fix_tilt, fix_exposure, whiten, color_cutout, background, matte) {
        let deferred10_0;
        let deferred10_1;
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16) >>> 0;
            const val4 = +face_index;
            const val5 = +fix_tilt;
            const val6 = +fix_exposure;
            const val7 = +whiten;
            const val8 = +color_cutout;
            let ptr0, len0, ptr1, len1, ptr2, len2;
            try {
                ptr0 = passStringToWasm0(rule_id, wasm.__wbindgen_export2, wasm.__wbindgen_export4);
                len0 = WASM_VECTOR_LEN;
                ptr1 = passStringToWasm0(background, wasm.__wbindgen_export2, wasm.__wbindgen_export4);
                len1 = WASM_VECTOR_LEN;
                ptr2 = passArray8ToWasm0(matte, wasm.__wbindgen_export2);
                len2 = WASM_VECTOR_LEN;
            } catch (e) {
                if (ptr2) { wasm.__wbindgen_export(ptr2, len2 * 1, 1); }
                if (ptr1) { wasm.__wbindgen_export(ptr1, len1 * 1, 1); }
                if (ptr0) { wasm.__wbindgen_export(ptr0, len0 * 1, 1); }
                throw e;
            }
            wasm.passport_prepare(retptr, this.__wbg_ptr, ptr0, len0, val4, val5, val6, val7, val8, ptr1, len1, ptr2, len2);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            var r2 = getDataViewMemory0().getInt32(retptr + 4 * 2, true);
            var r3 = getDataViewMemory0().getInt32(retptr + 4 * 3, true);
            var ptr9 = r0;
            var len9 = r1;
            if (r3) {
                ptr9 = 0; len9 = 0;
                throw takeFromExternrefTable0(r2);
            }
            deferred10_0 = ptr9;
            deferred10_1 = len9;
            return getStringFromWasm0(ptr9, len9);
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
            wasm.__wbindgen_export(deferred10_0, deferred10_1, 1);
        }
    }
    /**
     * Render at exact pixels; returns RGBA. `report()` has the checks.
     * @param {number} w
     * @param {number} h
     * @returns {Uint8Array}
     */
    render(w, h) {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16) >>> 0;
            wasm.passport_render(retptr, this.__wbg_ptr, w, h);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            var r2 = getDataViewMemory0().getInt32(retptr + 4 * 2, true);
            var r3 = getDataViewMemory0().getInt32(retptr + 4 * 3, true);
            if (r3) {
                throw takeFromExternrefTable0(r2);
            }
            var v1 = getArrayU8FromWasm0(r0, r1).slice();
            wasm.__wbindgen_export(r0, r1 * 1, 1);
            return v1;
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
     * @returns {string}
     */
    report() {
        let deferred1_0;
        let deferred1_1;
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16) >>> 0;
            wasm.passport_report(retptr, this.__wbg_ptr);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            deferred1_0 = r0;
            deferred1_1 = r1;
            return getStringFromWasm0(r0, r1);
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
            wasm.__wbindgen_export(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * Pixels of the output size for a rule: the portal's own pixels, or millimetres at `dpi` (0 uses the rule's).
     * @param {string} rule_id
     * @param {number} dpi
     * @returns {Uint32Array}
     */
    static rule_pixels(rule_id, dpi) {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16) >>> 0;
            const val2 = +dpi;
            const ptr0 = passStringToWasm0(rule_id, wasm.__wbindgen_export2, wasm.__wbindgen_export4);
            const len0 = WASM_VECTOR_LEN;
            wasm.passport_rule_pixels(retptr, ptr0, len0, val2);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            var r2 = getDataViewMemory0().getInt32(retptr + 4 * 2, true);
            var r3 = getDataViewMemory0().getInt32(retptr + 4 * 3, true);
            if (r3) {
                throw takeFromExternrefTable0(r2);
            }
            var v3 = getArrayU32FromWasm0(r0, r1).slice();
            wasm.__wbindgen_export(r0, r1 * 4, 4);
            return v3;
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
     * @returns {Uint32Array}
     */
    size() {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16) >>> 0;
            wasm.passport_size(retptr, this.__wbg_ptr);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            var v1 = getArrayU32FromWasm0(r0, r1).slice();
            wasm.__wbindgen_export(r0, r1 * 4, 4);
            return v1;
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
}
if (Symbol.dispose) Passport.prototype[Symbol.dispose] = Passport.prototype.free;

/**
 * A print sheet of passport-style photos.
 */
export class SheetMaker {
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        SheetMakerFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_sheetmaker_free(ptr, 0);
    }
    /**
     * @param {Uint8Array | number[]} rgba
     * @param {number} w
     * @param {number} h
     */
    add_photo(rgba, w, h) {
        const val2 = +w;
        const val3 = +h;
        const ptr0 = passArray8ToWasm0(rgba, wasm.__wbindgen_export2);
        const len0 = WASM_VECTOR_LEN;
        wasm.sheetmaker_add_photo(this.__wbg_ptr, ptr0, len0, val2, val3);
    }
    /**
     * @returns {number}
     */
    height() {
        const ret = wasm.sheetmaker_height(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {string}
     */
    info() {
        let deferred1_0;
        let deferred1_1;
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16) >>> 0;
            wasm.sheetmaker_info(retptr, this.__wbg_ptr);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            deferred1_0 = r0;
            deferred1_1 = r1;
            return getStringFromWasm0(r0, r1);
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
            wasm.__wbindgen_export(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * Paper: 4x6, 5x7, 6x8, a4, a5, a6, letter. `copies` 0 fills the sheet. Returns RGBA of the sheet.
     * @param {string} paper
     * @param {number} cell_w
     * @param {number} cell_h
     * @param {number} margin
     * @param {number} gutter
     * @param {number} copies
     * @param {boolean} cut_marks
     * @param {boolean} rotate
     * @param {number} dpi
     * @returns {Uint8Array}
     */
    make(paper, cell_w, cell_h, margin, gutter, copies, cut_marks, rotate, dpi) {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16) >>> 0;
            const val2 = +cell_w;
            const val3 = +cell_h;
            const val4 = +margin;
            const val5 = +gutter;
            const val6 = +copies;
            const val7 = +cut_marks;
            const val8 = +rotate;
            const val9 = +dpi;
            const ptr0 = passStringToWasm0(paper, wasm.__wbindgen_export2, wasm.__wbindgen_export4);
            const len0 = WASM_VECTOR_LEN;
            wasm.sheetmaker_make(retptr, this.__wbg_ptr, ptr0, len0, val2, val3, val4, val5, val6, val7, val8, val9);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            var r2 = getDataViewMemory0().getInt32(retptr + 4 * 2, true);
            var r3 = getDataViewMemory0().getInt32(retptr + 4 * 3, true);
            if (r3) {
                throw takeFromExternrefTable0(r2);
            }
            var v10 = getArrayU8FromWasm0(r0, r1).slice();
            wasm.__wbindgen_export(r0, r1 * 1, 1);
            return v10;
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    constructor() {
        const ret = wasm.sheetmaker_new();
        this.__wbg_ptr = ret;
        SheetMakerFinalization.register(this, this.__wbg_ptr, this);
        return this;
    }
    /**
     * @returns {number}
     */
    width() {
        const ret = wasm.sheetmaker_width(this.__wbg_ptr);
        return ret >>> 0;
    }
}
if (Symbol.dispose) SheetMaker.prototype[Symbol.dispose] = SheetMaker.prototype.free;

/**
 * Clean a photo of a signature. `ink`: keep, black or blue. `size`: 0 x 0 keeps the signature's own size.
 * Returns RGBA of the finished picture (transparent, or on white); `signature_notes()` has what was done.
 */
export class Signature {
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        SignatureFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_signature_free(ptr, 0);
    }
    /**
     * @returns {number}
     */
    height() {
        const ret = wasm.signature_height(this.__wbg_ptr);
        return ret >>> 0;
    }
    constructor() {
        const ret = wasm.signature_new();
        this.__wbg_ptr = ret;
        SignatureFinalization.register(this, this.__wbg_ptr, this);
        return this;
    }
    /**
     * @returns {string}
     */
    notes() {
        let deferred1_0;
        let deferred1_1;
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16) >>> 0;
            wasm.signature_notes(retptr, this.__wbg_ptr);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            deferred1_0 = r0;
            deferred1_1 = r1;
            return getStringFromWasm0(r0, r1);
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
            wasm.__wbindgen_export(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * @param {Uint8Array | number[]} rgba
     * @param {number} w
     * @param {number} h
     * @param {string} ink
     * @param {number} size_w
     * @param {number} size_h
     * @param {boolean} white
     * @param {boolean} remove_lines
     * @param {boolean} thicken
     * @returns {Uint8Array}
     */
    run(rgba, w, h, ink, size_w, size_h, white, remove_lines, thicken) {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16) >>> 0;
            const val3 = +w;
            const val4 = +h;
            const val5 = +size_w;
            const val6 = +size_h;
            const val7 = +white;
            const val8 = +remove_lines;
            const val9 = +thicken;
            let ptr0, len0, ptr1, len1;
            try {
                ptr0 = passArray8ToWasm0(rgba, wasm.__wbindgen_export2);
                len0 = WASM_VECTOR_LEN;
                ptr1 = passStringToWasm0(ink, wasm.__wbindgen_export2, wasm.__wbindgen_export4);
                len1 = WASM_VECTOR_LEN;
            } catch (e) {
                if (ptr1) { wasm.__wbindgen_export(ptr1, len1 * 1, 1); }
                if (ptr0) { wasm.__wbindgen_export(ptr0, len0 * 1, 1); }
                throw e;
            }
            wasm.signature_run(retptr, this.__wbg_ptr, ptr0, len0, val3, val4, ptr1, len1, val5, val6, val7, val8, val9);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            var r2 = getDataViewMemory0().getInt32(retptr + 4 * 2, true);
            var r3 = getDataViewMemory0().getInt32(retptr + 4 * 3, true);
            if (r3) {
                throw takeFromExternrefTable0(r2);
            }
            var v10 = getArrayU8FromWasm0(r0, r1).slice();
            wasm.__wbindgen_export(r0, r1 * 1, 1);
            return v10;
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
     * @returns {number}
     */
    width() {
        const ret = wasm.signature_width(this.__wbg_ptr);
        return ret >>> 0;
    }
}
if (Symbol.dispose) Signature.prototype[Symbol.dispose] = Signature.prototype.free;

/**
 * @returns {string}
 */
export function passport_rules() {
    let deferred1_0;
    let deferred1_1;
    try {
        const retptr = wasm.__wbindgen_add_to_stack_pointer(-16) >>> 0;
        wasm.passport_rules(retptr);
        var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
        var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
        deferred1_0 = r0;
        deferred1_1 = r1;
        return getStringFromWasm0(r0, r1);
    } finally {
        wasm.__wbindgen_add_to_stack_pointer(16);
        wasm.__wbindgen_export(deferred1_0, deferred1_1, 1);
    }
}
function __wbg_get_imports() {
    const import0 = {
        __proto__: null,
        __wbg_Error_208446807d0d1e30: function(arg0, arg1) {
            const ret = Error(getStringFromWasm0(arg0, arg1));
            return ret;
        },
        __wbg___wbindgen_throw_4c88d8bf264fd984: function(arg0, arg1) {
            throw new Error(getStringFromWasm0(arg0, arg1));
        },
        __wbindgen_init_externref_table: function() {
            const table = wasm.__wbindgen_externrefs;
            table.set(0, undefined);
            table.set(1024, undefined);
            table.set(1025, null);
            table.set(1026, true);
            table.set(1027, false);
        },
    };
    return {
        __proto__: null,
        "./panoply_web_id_bg.js": import0,
    };
}

const PassportFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_passport_free(ptr, 1));
const SheetMakerFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_sheetmaker_free(ptr, 1));
const SignatureFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_signature_free(ptr, 1));

function getArrayF32FromWasm0(ptr, len) {
    ptr = ptr >>> 0;
    return getFloat32ArrayMemory0().subarray(ptr / 4, ptr / 4 + len);
}

function getArrayU32FromWasm0(ptr, len) {
    ptr = ptr >>> 0;
    return getUint32ArrayMemory0().subarray(ptr / 4, ptr / 4 + len);
}

function getArrayU8FromWasm0(ptr, len) {
    ptr = ptr >>> 0;
    return getUint8ArrayMemory0().subarray(ptr / 1, ptr / 1 + len);
}

let cachedDataViewMemory0 = null;
function getDataViewMemory0() {
    if (cachedDataViewMemory0 === null || cachedDataViewMemory0.buffer.detached === true || (cachedDataViewMemory0.buffer.detached === undefined && cachedDataViewMemory0.buffer !== wasm.memory.buffer)) {
        cachedDataViewMemory0 = new DataView(wasm.memory.buffer);
    }
    return cachedDataViewMemory0;
}

let cachedFloat32ArrayMemory0 = null;
function getFloat32ArrayMemory0() {
    if (cachedFloat32ArrayMemory0 === null || cachedFloat32ArrayMemory0.byteLength === 0) {
        cachedFloat32ArrayMemory0 = new Float32Array(wasm.memory.buffer);
    }
    return cachedFloat32ArrayMemory0;
}

function getStringFromWasm0(ptr, len) {
    ptr = ptr >>> 0;
    return decodeText(getUint8ArrayMemory0().subarray(ptr, ptr + len));
}

let cachedUint32ArrayMemory0 = null;
function getUint32ArrayMemory0() {
    if (cachedUint32ArrayMemory0 === null || cachedUint32ArrayMemory0.byteLength === 0) {
        cachedUint32ArrayMemory0 = new Uint32Array(wasm.memory.buffer);
    }
    return cachedUint32ArrayMemory0;
}

let cachedUint8ArrayMemory0 = null;
function getUint8ArrayMemory0() {
    if (cachedUint8ArrayMemory0 === null || cachedUint8ArrayMemory0.byteLength === 0) {
        cachedUint8ArrayMemory0 = new Uint8Array(wasm.memory.buffer);
    }
    return cachedUint8ArrayMemory0;
}

function passArray8ToWasm0(arg, malloc) {
    const ptr = malloc(arg.length * 1, 1) >>> 0;
    getUint8ArrayMemory0().set(arg, ptr / 1);
    WASM_VECTOR_LEN = arg.length;
    return ptr;
}

function passArrayF32ToWasm0(arg, malloc) {
    const ptr = malloc(arg.length * 4, 4) >>> 0;
    getFloat32ArrayMemory0().set(arg, ptr / 4);
    WASM_VECTOR_LEN = arg.length;
    return ptr;
}

function passStringToWasm0(arg, malloc, realloc) {
    if (realloc === undefined) {
        const buf = cachedTextEncoder.encode(arg);
        const ptr = malloc(buf.length, 1) >>> 0;
        getUint8ArrayMemory0().subarray(ptr, ptr + buf.length).set(buf);
        WASM_VECTOR_LEN = buf.length;
        return ptr;
    }

    let len = arg.length;
    let ptr = malloc(len, 1) >>> 0;

    const mem = getUint8ArrayMemory0();

    let offset = 0;

    for (; offset < len; offset++) {
        const code = arg.charCodeAt(offset);
        if (code > 0x7F) break;
        mem[ptr + offset] = code;
    }
    if (offset !== len) {
        if (offset !== 0) {
            arg = arg.slice(offset);
        }
        ptr = realloc(ptr, len, len = offset + arg.length * 3, 1) >>> 0;
        const view = getUint8ArrayMemory0().subarray(ptr + offset, ptr + len);
        const ret = cachedTextEncoder.encodeInto(arg, view);

        offset += ret.written;
        ptr = realloc(ptr, len, offset, 1) >>> 0;
    }

    WASM_VECTOR_LEN = offset;
    return ptr;
}

function takeFromExternrefTable0(idx) {
    const value = wasm.__wbindgen_externrefs.get(idx);
    wasm.__wbindgen_export3(idx);
    return value;
}

let cachedTextDecoder = new TextDecoder('utf-8', { ignoreBOM: true });
cachedTextDecoder.decode();
const MAX_SAFARI_DECODE_BYTES = 2146435072;
let numBytesDecoded = 0;
function decodeText(bytes) {
    numBytesDecoded += bytes.length;
    if (numBytesDecoded >= MAX_SAFARI_DECODE_BYTES) {
        cachedTextDecoder = new TextDecoder('utf-8', { ignoreBOM: true });
        cachedTextDecoder.decode();
        numBytesDecoded = bytes.length;
    }
    return cachedTextDecoder.decode(bytes);
}

const cachedTextEncoder = new TextEncoder();

if (!('encodeInto' in cachedTextEncoder)) {
    cachedTextEncoder.encodeInto = function (arg, view) {
        const buf = cachedTextEncoder.encode(arg);
        view.set(buf);
        return {
            read: arg.length,
            written: buf.length
        };
    };
}

let WASM_VECTOR_LEN = 0;

let wasmModule, wasmInstance, wasm;
function __wbg_finalize_init(instance, module) {
    wasmInstance = instance;
    wasm = instance.exports;
    wasmModule = module;
    cachedDataViewMemory0 = null;
    cachedFloat32ArrayMemory0 = null;
    cachedUint32ArrayMemory0 = null;
    cachedUint8ArrayMemory0 = null;
    wasm.__wbindgen_start();
    return wasm;
}

async function __wbg_load(module, imports) {
    if (typeof Response === 'function' && module instanceof Response) {
        if (!module.ok) {
            throw new Error(`failed to fetch Wasm: ${module.status} ${module.statusText} fetching '${module.url}'`);
        }

        if (typeof WebAssembly.instantiateStreaming === 'function') {
            try {
                return await WebAssembly.instantiateStreaming(module, imports);
            } catch (e) {
                const validResponse = expectedResponseType(module.type);

                if (validResponse && module.headers.get('Content-Type') !== 'application/wasm') {
                    console.warn("`WebAssembly.instantiateStreaming` failed because your server does not serve Wasm with `application/wasm` MIME type. Falling back to `WebAssembly.instantiate` which is slower. Original error:\n", e);

                } else { throw e; }
            }
        }

        const bytes = await module.arrayBuffer();
        return await WebAssembly.instantiate(bytes, imports);
    } else {
        const instance = await WebAssembly.instantiate(module, imports);

        if (instance instanceof WebAssembly.Instance) {
            return { instance, module };
        } else {
            return instance;
        }
    }

    function expectedResponseType(type) {
        switch (type) {
            case 'basic': case 'cors': case 'default': return true;
        }
        return false;
    }
}

function initSync(module) {
    if (wasm !== undefined) return wasm;


    if (module !== undefined) {
        if (Object.getPrototypeOf(module) === Object.prototype) {
            ({module} = module)
        } else {
            console.warn('using deprecated parameters for `initSync()`; pass a single object instead')
        }
    }

    const imports = __wbg_get_imports();
    if (!(module instanceof WebAssembly.Module)) {
        module = new WebAssembly.Module(module);
    }
    const instance = new WebAssembly.Instance(module, imports);
    return __wbg_finalize_init(instance, module);
}

let __wbg_init_promise;
async function __wbg_init(module_or_path) {
    if (wasm !== undefined) return wasm;
    if (__wbg_init_promise === undefined) {
        __wbg_init_promise = __wbg_init_once(module_or_path);
        __wbg_init_promise.catch(() => { __wbg_init_promise = undefined; });
    }
    return __wbg_init_promise;
}

async function __wbg_init_once(module_or_path) {

    if (module_or_path !== undefined) {
        if (Object.getPrototypeOf(module_or_path) === Object.prototype) {
            ({module_or_path} = module_or_path)
        } else {
            console.warn('using deprecated parameters for the initialization function; pass a single object instead')
        }
    }

    if (module_or_path === undefined) {
        module_or_path = new URL('panoply_web_id_bg.wasm', import.meta.url);
    }
    const imports = __wbg_get_imports();

    if (typeof module_or_path === 'string' || (typeof Request === 'function' && module_or_path instanceof Request) || (typeof URL === 'function' && module_or_path instanceof URL)) {
        module_or_path = fetch(module_or_path);
    }

    const { instance, module } = await __wbg_load(await module_or_path, imports);

    return __wbg_finalize_init(instance, module);
}

export { initSync, __wbg_init as default };
