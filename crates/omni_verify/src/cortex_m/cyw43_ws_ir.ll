; Standard Runtime & libc Declarations
declare i8* @malloc(i64)
declare void @free(i8*)
declare i32 @printf(i8*, ...)

; --- OmniScript Framework Runtime Declarations ---
; Standard POSIX libc & System Declarations
declare i32 @sleep(i32)
declare i32 @usleep(i32)

; Printing, Time & Hashing helpers
declare void @__omni_print_str(i8*)
declare void @__omni_print_str_inline(i8*)
declare void @__omni_print_str_raw(i8*)
declare void @__omni_print_int(i64)
declare void @__omni_print_float(double)
declare i8* @__omni_input(i8*)
declare void @exit(i32)
declare i32 @strcmp(i8*, i8*)
declare i8* @__omni_str_concat(i8*, i8*)
declare i64 @__omni_str_len(i8*)
declare i8* @__omni_str_char_at(i8*, i64)
declare i64 @__omni_str_byte_at(i8*, i64)
declare i64 @__omni_str_ord(i8*)
declare i8* @__omni_str_chr(i64)
declare i8* @__omni_str_slice_step(i8*, i64, i64, i64, i64, i64)
declare i64 @__omni_str_contains(i8*, i8*)
declare i64 @__omni_str_starts_with(i8*, i8*)
declare i64 @__omni_str_ends_with(i8*, i8*)
declare i64 @__omni_str_find(i8*, i8*)
declare i8* @__omni_str_upper(i8*)
declare i8* @__omni_str_lower(i8*)
declare i8* @__omni_str_trim(i8*)
declare i8* @__omni_str_replace(i8*, i8*, i8*)
declare i8* @__omni_str_split(i8*, i8*)
declare i8* @__omni_str_to_chars(i8*)
declare i8* @__omni_int_to_str(i64)
declare i8* @__omni_float_to_str(double)
declare double @__omni_c_str_to_f64(i8*)
declare i64 @__omni_c_str_to_i64(i8*)
declare void @__omni_sleep_s(i64)
declare void @__omni_sleep_ms(i64)
declare i64 @__omni_random_int()
declare i64 @__omni_random_range(i64, i64)
declare i64 @__omni_ipow64(i64, i64)
declare double @llvm.pow.f64(double, double)
declare void @fast_hash_block(i8*, i64, i8*)

; Thread-Local Stack-Unwinding Exception Runtime (setjmp / longjmp)
declare i32 @_setjmp(i8*) returns_twice
declare i8* @__omni_try_push()
declare void @__omni_try_pop()
declare void @__omni_throw(i64, i8*, i8*)
declare void @__omni_rethrow()
declare i8* @__omni_catch_msg()
declare i8* @__omni_catch_type()
declare i64 @__omni_catch_code()
declare i32 @__omni_catch_matches(i8*)

; Arena Allocator
declare i8* @__omni_arena_create(i64)
declare i8* @__omni_arena_alloc(i8*, i64, i64)
declare i64 @__omni_arena_mark(i8*)
declare void @__omni_arena_rewind(i8*, i64)
declare void @__omni_arena_reset(i8*)
declare void @__omni_arena_destroy(i8*)

; Channels (Go Concurrency Emulation)
declare i8* @__omni_chan_make(i64, i64)
declare i1 @__omni_chan_send(i8*, i8*)
declare i1 @__omni_chan_recv(i8*, i8*)
declare i1 @__omni_chan_send_int(i8*, i64)
declare i64 @__omni_chan_recv_int(i8*)
declare i64 @__omni_chan_len(i8*)
declare void @__omni_chan_close(i8*)
declare void @__omni_chan_destroy(i8*)

; Fiber / M:N Goroutine Runtime
declare void @__omni_fiber_init()
declare i8* @__omni_fiber_create(void (i8*)*, i8*, i64)
declare void @__omni_fiber_yield()
declare void @__omni_fiber_join_all()
declare void @__omni_fiber_destroy(i8*)
declare void @__omni_go_spawn(void (i8*)*, i8*)
declare void @__omni_thread_spawn(void (i8*)*, i8*)
declare void @__omni_thread_join_all()
declare i8* @__omni_fiber_run_on_main_stack(i8* (i8*)*, i8*)

; Polyglot Collections (Slices / Lists)
declare i8* @__omni_slice_create(i64)
declare void @__omni_slice_append(i8*, i64)
declare i64 @__omni_slice_get(i8*, i64)
declare void @__omni_slice_set(i8*, i64, i64)
declare i64 @__omni_slice_len(i8*)
declare i64 @__omni_slice_pop(i8*)
declare i8* @__omni_slice_subslice(i8*, i64, i64, i64, i64, i64)
declare i64 @__omni_slice_contains(i8*, i64, i64)
declare void @__omni_slice_print(i8*)
declare void @__omni_slice_destroy(i8*)

; Polyglot Collections (Hash Maps / Dicts)
declare i8* @__omni_map_create(i64)
declare void @__omni_map_set(i8*, i8*, i64)
declare i64 @__omni_map_get(i8*, i8*)
declare i1 @__omni_map_contains(i8*, i8*)
declare void @__omni_map_delete(i8*, i8*)
declare i64 @__omni_map_len(i8*)
declare void @__omni_map_destroy(i8*)
declare void @__omni_map_set_int(i8*, i64, i64)
declare i64 @__omni_map_get_int(i8*, i64)
declare i1 @__omni_map_contains_int(i8*, i64)
declare void @__omni_map_delete_int(i8*, i64)

; Perceus Refcounting & Fat Closures (`{ fn_ptr, env_ptr }`)
declare i8* @__omni_rc_retain(i8*)
declare void @__omni_rc_release(i8*)
declare i8* @__omni_closure_create(i8*, i8*)
declare void @__omni_closure_set_env_rc_mask(i8*, i64)
declare i64 @__omni_closure_is(i8*)
declare i8* @__omni_closure_fn(i8*)
declare i8* @__omni_closure_env(i8*)
declare void @__omni_closure_destroy(i8*)
declare i8* @__omni_obj_alloc(i64, i64)
declare void @__omni_obj_set_type(i8*, i64)
declare i64 @__omni_obj_get_type(i8*)

; Interpretive C-Frontier Short-Circuit Intrinsics & OS Socket Syscalls
declare i8* @__omni_c_http_get(i8*, i32*)
declare i8* @__omni_c_json_parse(i8*)
declare i8* @__omni_c_json_get(i8*, i8*)
declare i8* @__omni_obj_get(i8*, i8*)
declare i32 @__omni_c_to_float(i8*)
declare i32 @__omni_c_to_int(i8*)
declare i8* @__omni_c_return_str(i8*)
declare i8* @__omni_c_get_returned_str()
declare void @__omni_c_return_ptr(i8*)
declare i8* @__omni_c_get_returned_ptr()
declare i8* @__omni_c_consume_returned_ptr()
declare i8* @__omni_str_token(i8*, i32)
declare i8* @__omni_str_token_arena(i8*, i8*, i32)
declare i32 @__omni_os_socket_listen(i32)
declare i32 @__omni_os_socket_accept(i32)
declare i8* @__omni_os_socket_read(i32)
declare i8* @__omni_os_socket_read_arena(i8*, i32)
declare i32 @__omni_os_socket_write_http(i32, i32, i8*)
declare void @__omni_os_socket_close(i32)
declare i32 @__omni_os_socket_create(i32, i32, i32)
declare i32 @__omni_os_socket_bind(i32, i8*, i32)
declare i32 @__omni_os_socket_listen_fd(i32, i32)
declare i8* @__omni_os_socket_accept_tuple(i32)
declare i32 @__omni_os_socket_connect(i32, i8*, i32)
declare i8* @__omni_os_socket_getpeername(i32)
declare i32 @__omni_os_socket_send(i32, i8*)
declare i8* @__omni_os_socket_recv(i32, i32)
declare i8* @__omni_os_pipe_pair()

; Dynamically Harvested External Declarations
declare i64 @calloc(...)
declare i64 @cyw43_init()
declare i64 @cyw_ll_clm_ptr()
declare i64 @cyw_ll_gspi_read_bytes(i64, i64)
declare i64 @cyw_ll_gspi_write_bytes(i64, i64)
declare i64 @cyw_ll_gspi_write_u32(i64)
declare i64 @cyw_ll_pack_cmd(i64, i64, i64, i64, i64)
declare i64 @cyw_ll_read_reg(i64, i64, i64)
declare i64 @cyw_ll_sram_scratch()
declare i64 @cyw_ll_write_bp(i64, i64, i64)
declare i64 @cyw_ll_write_reg(i64, i64, i64, i64)
declare i64 @gpio_put(i64, i64)
declare i64 @mem_read8(i64)
declare i64 @mem_write8(i64, i64)
declare i64 @mmio_read32(i64)
declare i64 @mmio_write32(i64, i64)
declare i64 @sleep_ms(i64)
declare i64 @sleep_us(i64)
declare i64 @time_us()

@.str.cyw_0 = private unnamed_addr constant [1 x i8] c"\00", align 1
@.str.cyw_1 = private unnamed_addr constant [10 x i8] c"Exception\00", align 1
@.str.cyw_2 = private unnamed_addr constant [18 x i8] c"ZeroDivisionError\00", align 1
@.str.cyw_3 = private unnamed_addr constant [17 x i8] c"division by zero\00", align 1
@.str.cyw_4 = private unnamed_addr constant [37 x i8] c"258EAFA5-E914-47DA-95CA-C5AB0DC85B11\00", align 1
@.str.cyw_5 = private unnamed_addr constant [16 x i8] c"OMNI_PICO2W ip=\00", align 1
@.str.cyw_6 = private unnamed_addr constant [10 x i8] c" ws_port=\00", align 1
@.str.cyw_7 = private unnamed_addr constant [12 x i8] c"omni-pico2w\00", align 1
@.str.cyw_8 = private unnamed_addr constant [19 x i8] c"sec-websocket-key:\00", align 1
@.str.cyw_9 = private unnamed_addr constant [98 x i8] c"HTTP/1.1 101 Switching Protocols\0D\0AUpgrade: websocket\0D\0AConnection: Upgrade\0D\0ASec-WebSocket-Accept: \00", align 1
@.str.cyw_10 = private unnamed_addr constant [5 x i8] c"\0D\0A\0D\0A\00", align 1
@.str.cyw_11 = private unnamed_addr constant [8 x i8] c"gpioout\00", align 1
@.str.cyw_12 = private unnamed_addr constant [8 x i8] c"clmload\00", align 1
@.str.cyw_13 = private unnamed_addr constant [8 x i8] c"country\00", align 1
@.str.cyw_14 = private unnamed_addr constant [11 x i8] c"bus:txglom\00", align 1
@.str.cyw_15 = private unnamed_addr constant [6 x i8] c"apsta\00", align 1
@.str.cyw_16 = private unnamed_addr constant [15 x i8] c"ampdu_ba_wsize\00", align 1
@.str.cyw_17 = private unnamed_addr constant [11 x i8] c"ampdu_mpdu\00", align 1
@.str.cyw_18 = private unnamed_addr constant [16 x i8] c"ampdu_rx_factor\00", align 1
@.str.cyw_19 = private unnamed_addr constant [18 x i8] c"bsscfg:event_msgs\00", align 1
@.str.cyw_20 = private unnamed_addr constant [14 x i8] c"cur_etheraddr\00", align 1
@.str.cyw_21 = private unnamed_addr constant [14 x i8] c"pm2_sleep_ret\00", align 1
@.str.cyw_22 = private unnamed_addr constant [11 x i8] c"bcn_li_bcn\00", align 1
@.str.cyw_23 = private unnamed_addr constant [12 x i8] c"bcn_li_dtim\00", align 1
@.str.cyw_24 = private unnamed_addr constant [13 x i8] c"assoc_listen\00", align 1
@.str.cyw_25 = private unnamed_addr constant [15 x i8] c"bsscfg:sup_wpa\00", align 1
@.str.cyw_26 = private unnamed_addr constant [23 x i8] c"bsscfg:sup_wpa2_eapver\00", align 1
@.str.cyw_27 = private unnamed_addr constant [19 x i8] c"bsscfg:sup_wpa_tmo\00", align 1
@.str.cyw_28 = private unnamed_addr constant [4 x i8] c"mfp\00", align 1

define i64 @__omni_cyw_priv_r8(i64 %arg0, i64 %arg1, i64 %arg2, i64 %arg3, i64 %arg4, i64 %arg5) {
entry:
  %param_p_1 = alloca i64
  %param_unset_p_0 = icmp eq i64 %arg0, -581005069656919554
  %param_dsel_p_0 = select i1 %param_unset_p_0, i64 0, i64 %arg0
  store i64 %param_dsel_p_0, i64* %param_p_1
  %t64_2 = load i64, i64* %param_p_1
  %ext_i64_3 = call i64 @mem_read8(i64 %t64_2)
  %t_4 = and i64 %ext_i64_3, 255
  ret i64 %t_4
}


define void @__omni_trampoline_r8(i8* %raw_arg) {
entry:
  %raw_int = ptrtoint i8* %raw_arg to i64
  %res = call i64 @__omni_cyw_priv_r8(i64 %raw_int, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  ret void
}

define void @__omni_trampoline_pack_r8(i8* %raw_pack) {
entry:
  %p64 = bitcast i8* %raw_pack to i64*
  %p0 = getelementptr inbounds i64, i64* %p64, i64 0
  %a0 = load i64, i64* %p0
  %p1 = getelementptr inbounds i64, i64* %p64, i64 1
  %a1 = load i64, i64* %p1
  %p2 = getelementptr inbounds i64, i64* %p64, i64 2
  %a2 = load i64, i64* %p2
  %p3 = getelementptr inbounds i64, i64* %p64, i64 3
  %a3 = load i64, i64* %p3
  %p4 = getelementptr inbounds i64, i64* %p64, i64 4
  %a4 = load i64, i64* %p4
  %p5 = getelementptr inbounds i64, i64* %p64, i64 5
  %a5 = load i64, i64* %p5
  call void @free(i8* %raw_pack)
  %res = call i64 @__omni_cyw_priv_r8(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5)
  ret void
}

define i64 @__omni_cyw_priv_w8(i64 %arg0, i64 %arg1, i64 %arg2, i64 %arg3, i64 %arg4, i64 %arg5) {
entry:
  %param_p_6 = alloca i64
  %param_v_8 = alloca i64
  %param_unset_p_5 = icmp eq i64 %arg0, -581005069656919554
  %param_dsel_p_5 = select i1 %param_unset_p_5, i64 0, i64 %arg0
  store i64 %param_dsel_p_5, i64* %param_p_6
  %param_unset_v_7 = icmp eq i64 %arg1, -581005069656919554
  %param_dsel_v_7 = select i1 %param_unset_v_7, i64 0, i64 %arg1
  store i64 %param_dsel_v_7, i64* %param_v_8
  %t64_9 = load i64, i64* %param_p_6
  %t64_10 = load i64, i64* %param_v_8
  %t_11 = and i64 %t64_10, 255
  %ext_i64_12 = call i64 @mem_write8(i64 %t64_9, i64 %t_11)
  ret i64 0
}


define void @__omni_trampoline_w8(i8* %raw_arg) {
entry:
  %raw_int = ptrtoint i8* %raw_arg to i64
  %res = call i64 @__omni_cyw_priv_w8(i64 %raw_int, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  ret void
}

define void @__omni_trampoline_pack_w8(i8* %raw_pack) {
entry:
  %p64 = bitcast i8* %raw_pack to i64*
  %p0 = getelementptr inbounds i64, i64* %p64, i64 0
  %a0 = load i64, i64* %p0
  %p1 = getelementptr inbounds i64, i64* %p64, i64 1
  %a1 = load i64, i64* %p1
  %p2 = getelementptr inbounds i64, i64* %p64, i64 2
  %a2 = load i64, i64* %p2
  %p3 = getelementptr inbounds i64, i64* %p64, i64 3
  %a3 = load i64, i64* %p3
  %p4 = getelementptr inbounds i64, i64* %p64, i64 4
  %a4 = load i64, i64* %p4
  %p5 = getelementptr inbounds i64, i64* %p64, i64 5
  %a5 = load i64, i64* %p5
  call void @free(i8* %raw_pack)
  %res = call i64 @__omni_cyw_priv_w8(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5)
  ret void
}

define i64 @__omni_cyw_priv_r16_be(i64 %arg0, i64 %arg1, i64 %arg2, i64 %arg3, i64 %arg4, i64 %arg5) {
entry:
  %param_p_14 = alloca i64
  %param_unset_p_13 = icmp eq i64 %arg0, -581005069656919554
  %param_dsel_p_13 = select i1 %param_unset_p_13, i64 0, i64 %arg0
  store i64 %param_dsel_p_13, i64* %param_p_14
  %t64_15 = load i64, i64* %param_p_14
  %fn_ret_16 = call i64 @__omni_cyw_priv_r8(i64 %t64_15, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t_17 = shl i64 %fn_ret_16, 8
  %t64_18 = load i64, i64* %param_p_14
  %t_19 = add i64 %t64_18, 1
  %fn_ret_20 = call i64 @__omni_cyw_priv_r8(i64 %t_19, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t_21 = or i64 %t_17, %fn_ret_20
  %t_22 = and i64 %t_21, 65535
  ret i64 %t_22
}


define void @__omni_trampoline_r16_be(i8* %raw_arg) {
entry:
  %raw_int = ptrtoint i8* %raw_arg to i64
  %res = call i64 @__omni_cyw_priv_r16_be(i64 %raw_int, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  ret void
}

define void @__omni_trampoline_pack_r16_be(i8* %raw_pack) {
entry:
  %p64 = bitcast i8* %raw_pack to i64*
  %p0 = getelementptr inbounds i64, i64* %p64, i64 0
  %a0 = load i64, i64* %p0
  %p1 = getelementptr inbounds i64, i64* %p64, i64 1
  %a1 = load i64, i64* %p1
  %p2 = getelementptr inbounds i64, i64* %p64, i64 2
  %a2 = load i64, i64* %p2
  %p3 = getelementptr inbounds i64, i64* %p64, i64 3
  %a3 = load i64, i64* %p3
  %p4 = getelementptr inbounds i64, i64* %p64, i64 4
  %a4 = load i64, i64* %p4
  %p5 = getelementptr inbounds i64, i64* %p64, i64 5
  %a5 = load i64, i64* %p5
  call void @free(i8* %raw_pack)
  %res = call i64 @__omni_cyw_priv_r16_be(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5)
  ret void
}

define i64 @__omni_cyw_priv_w16_be(i64 %arg0, i64 %arg1, i64 %arg2, i64 %arg3, i64 %arg4, i64 %arg5) {
entry:
  %param_p_24 = alloca i64
  %param_v_26 = alloca i64
  %param_unset_p_23 = icmp eq i64 %arg0, -581005069656919554
  %param_dsel_p_23 = select i1 %param_unset_p_23, i64 0, i64 %arg0
  store i64 %param_dsel_p_23, i64* %param_p_24
  %param_unset_v_25 = icmp eq i64 %arg1, -581005069656919554
  %param_dsel_v_25 = select i1 %param_unset_v_25, i64 0, i64 %arg1
  store i64 %param_dsel_v_25, i64* %param_v_26
  %t64_27 = load i64, i64* %param_p_24
  %t64_28 = load i64, i64* %param_v_26
  %t_29 = ashr i64 %t64_28, 8
  %t_30 = and i64 %t_29, 255
  %fn_ret_31 = call i64 @__omni_cyw_priv_w8(i64 %t64_27, i64 %t_30, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_32 = load i64, i64* %param_p_24
  %t_33 = add i64 %t64_32, 1
  %t64_34 = load i64, i64* %param_v_26
  %t_35 = and i64 %t64_34, 255
  %fn_ret_36 = call i64 @__omni_cyw_priv_w8(i64 %t_33, i64 %t_35, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  ret i64 0
}


define void @__omni_trampoline_w16_be(i8* %raw_arg) {
entry:
  %raw_int = ptrtoint i8* %raw_arg to i64
  %res = call i64 @__omni_cyw_priv_w16_be(i64 %raw_int, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  ret void
}

define void @__omni_trampoline_pack_w16_be(i8* %raw_pack) {
entry:
  %p64 = bitcast i8* %raw_pack to i64*
  %p0 = getelementptr inbounds i64, i64* %p64, i64 0
  %a0 = load i64, i64* %p0
  %p1 = getelementptr inbounds i64, i64* %p64, i64 1
  %a1 = load i64, i64* %p1
  %p2 = getelementptr inbounds i64, i64* %p64, i64 2
  %a2 = load i64, i64* %p2
  %p3 = getelementptr inbounds i64, i64* %p64, i64 3
  %a3 = load i64, i64* %p3
  %p4 = getelementptr inbounds i64, i64* %p64, i64 4
  %a4 = load i64, i64* %p4
  %p5 = getelementptr inbounds i64, i64* %p64, i64 5
  %a5 = load i64, i64* %p5
  call void @free(i8* %raw_pack)
  %res = call i64 @__omni_cyw_priv_w16_be(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5)
  ret void
}

define i64 @__omni_cyw_priv_r16_le(i64 %arg0, i64 %arg1, i64 %arg2, i64 %arg3, i64 %arg4, i64 %arg5) {
entry:
  %param_p_38 = alloca i64
  %param_unset_p_37 = icmp eq i64 %arg0, -581005069656919554
  %param_dsel_p_37 = select i1 %param_unset_p_37, i64 0, i64 %arg0
  store i64 %param_dsel_p_37, i64* %param_p_38
  %t64_39 = load i64, i64* %param_p_38
  %fn_ret_40 = call i64 @__omni_cyw_priv_r8(i64 %t64_39, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_41 = load i64, i64* %param_p_38
  %t_42 = add i64 %t64_41, 1
  %fn_ret_43 = call i64 @__omni_cyw_priv_r8(i64 %t_42, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t_44 = shl i64 %fn_ret_43, 8
  %t_45 = or i64 %fn_ret_40, %t_44
  %t_46 = and i64 %t_45, 65535
  ret i64 %t_46
}


define void @__omni_trampoline_r16_le(i8* %raw_arg) {
entry:
  %raw_int = ptrtoint i8* %raw_arg to i64
  %res = call i64 @__omni_cyw_priv_r16_le(i64 %raw_int, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  ret void
}

define void @__omni_trampoline_pack_r16_le(i8* %raw_pack) {
entry:
  %p64 = bitcast i8* %raw_pack to i64*
  %p0 = getelementptr inbounds i64, i64* %p64, i64 0
  %a0 = load i64, i64* %p0
  %p1 = getelementptr inbounds i64, i64* %p64, i64 1
  %a1 = load i64, i64* %p1
  %p2 = getelementptr inbounds i64, i64* %p64, i64 2
  %a2 = load i64, i64* %p2
  %p3 = getelementptr inbounds i64, i64* %p64, i64 3
  %a3 = load i64, i64* %p3
  %p4 = getelementptr inbounds i64, i64* %p64, i64 4
  %a4 = load i64, i64* %p4
  %p5 = getelementptr inbounds i64, i64* %p64, i64 5
  %a5 = load i64, i64* %p5
  call void @free(i8* %raw_pack)
  %res = call i64 @__omni_cyw_priv_r16_le(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5)
  ret void
}

define i64 @__omni_cyw_priv_w16_le(i64 %arg0, i64 %arg1, i64 %arg2, i64 %arg3, i64 %arg4, i64 %arg5) {
entry:
  %param_p_48 = alloca i64
  %param_v_50 = alloca i64
  %param_unset_p_47 = icmp eq i64 %arg0, -581005069656919554
  %param_dsel_p_47 = select i1 %param_unset_p_47, i64 0, i64 %arg0
  store i64 %param_dsel_p_47, i64* %param_p_48
  %param_unset_v_49 = icmp eq i64 %arg1, -581005069656919554
  %param_dsel_v_49 = select i1 %param_unset_v_49, i64 0, i64 %arg1
  store i64 %param_dsel_v_49, i64* %param_v_50
  %t64_51 = load i64, i64* %param_p_48
  %t64_52 = load i64, i64* %param_v_50
  %t_53 = and i64 %t64_52, 255
  %fn_ret_54 = call i64 @__omni_cyw_priv_w8(i64 %t64_51, i64 %t_53, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_55 = load i64, i64* %param_p_48
  %t_56 = add i64 %t64_55, 1
  %t64_57 = load i64, i64* %param_v_50
  %t_58 = ashr i64 %t64_57, 8
  %t_59 = and i64 %t_58, 255
  %fn_ret_60 = call i64 @__omni_cyw_priv_w8(i64 %t_56, i64 %t_59, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  ret i64 0
}


define void @__omni_trampoline_w16_le(i8* %raw_arg) {
entry:
  %raw_int = ptrtoint i8* %raw_arg to i64
  %res = call i64 @__omni_cyw_priv_w16_le(i64 %raw_int, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  ret void
}

define void @__omni_trampoline_pack_w16_le(i8* %raw_pack) {
entry:
  %p64 = bitcast i8* %raw_pack to i64*
  %p0 = getelementptr inbounds i64, i64* %p64, i64 0
  %a0 = load i64, i64* %p0
  %p1 = getelementptr inbounds i64, i64* %p64, i64 1
  %a1 = load i64, i64* %p1
  %p2 = getelementptr inbounds i64, i64* %p64, i64 2
  %a2 = load i64, i64* %p2
  %p3 = getelementptr inbounds i64, i64* %p64, i64 3
  %a3 = load i64, i64* %p3
  %p4 = getelementptr inbounds i64, i64* %p64, i64 4
  %a4 = load i64, i64* %p4
  %p5 = getelementptr inbounds i64, i64* %p64, i64 5
  %a5 = load i64, i64* %p5
  call void @free(i8* %raw_pack)
  %res = call i64 @__omni_cyw_priv_w16_le(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5)
  ret void
}

define i64 @__omni_cyw_priv_r32_be(i64 %arg0, i64 %arg1, i64 %arg2, i64 %arg3, i64 %arg4, i64 %arg5) {
entry:
  %param_p_62 = alloca i64
  %var_b0_66 = alloca i64
  %var_b1_71 = alloca i64
  %var_b2_76 = alloca i64
  %var_b3_80 = alloca i64
  %param_unset_p_61 = icmp eq i64 %arg0, -581005069656919554
  %param_dsel_p_61 = select i1 %param_unset_p_61, i64 0, i64 %arg0
  store i64 %param_dsel_p_61, i64* %param_p_62
  call void @__omni_c_return_ptr(i8* null)
  %t64_63 = load i64, i64* %param_p_62
  %fn_ret_64 = call i64 @__omni_cyw_priv_r8(i64 %t64_63, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t_65 = shl i64 %fn_ret_64, 24
  store i64 %t_65, i64* %var_b0_66
  call void @__omni_c_return_ptr(i8* null)
  %t64_67 = load i64, i64* %param_p_62
  %t_68 = add i64 %t64_67, 1
  %fn_ret_69 = call i64 @__omni_cyw_priv_r8(i64 %t_68, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t_70 = shl i64 %fn_ret_69, 16
  store i64 %t_70, i64* %var_b1_71
  call void @__omni_c_return_ptr(i8* null)
  %t64_72 = load i64, i64* %param_p_62
  %t_73 = add i64 %t64_72, 2
  %fn_ret_74 = call i64 @__omni_cyw_priv_r8(i64 %t_73, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t_75 = shl i64 %fn_ret_74, 8
  store i64 %t_75, i64* %var_b2_76
  call void @__omni_c_return_ptr(i8* null)
  %t64_77 = load i64, i64* %param_p_62
  %t_78 = add i64 %t64_77, 3
  %fn_ret_79 = call i64 @__omni_cyw_priv_r8(i64 %t_78, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  store i64 %fn_ret_79, i64* %var_b3_80
  %t64_81 = load i64, i64* %var_b0_66
  %t64_82 = load i64, i64* %var_b1_71
  %t_83 = or i64 %t64_81, %t64_82
  %t64_84 = load i64, i64* %var_b2_76
  %t_85 = or i64 %t_83, %t64_84
  %t64_86 = load i64, i64* %var_b3_80
  %t_87 = or i64 %t_85, %t64_86
  %t_88 = and i64 %t_87, 4294967295
  ret i64 %t_88
}


define void @__omni_trampoline_r32_be(i8* %raw_arg) {
entry:
  %raw_int = ptrtoint i8* %raw_arg to i64
  %res = call i64 @__omni_cyw_priv_r32_be(i64 %raw_int, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  ret void
}

define void @__omni_trampoline_pack_r32_be(i8* %raw_pack) {
entry:
  %p64 = bitcast i8* %raw_pack to i64*
  %p0 = getelementptr inbounds i64, i64* %p64, i64 0
  %a0 = load i64, i64* %p0
  %p1 = getelementptr inbounds i64, i64* %p64, i64 1
  %a1 = load i64, i64* %p1
  %p2 = getelementptr inbounds i64, i64* %p64, i64 2
  %a2 = load i64, i64* %p2
  %p3 = getelementptr inbounds i64, i64* %p64, i64 3
  %a3 = load i64, i64* %p3
  %p4 = getelementptr inbounds i64, i64* %p64, i64 4
  %a4 = load i64, i64* %p4
  %p5 = getelementptr inbounds i64, i64* %p64, i64 5
  %a5 = load i64, i64* %p5
  call void @free(i8* %raw_pack)
  %res = call i64 @__omni_cyw_priv_r32_be(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5)
  ret void
}

define i64 @__omni_cyw_priv_w32_be(i64 %arg0, i64 %arg1, i64 %arg2, i64 %arg3, i64 %arg4, i64 %arg5) {
entry:
  %param_p_90 = alloca i64
  %param_v_92 = alloca i64
  %param_unset_p_89 = icmp eq i64 %arg0, -581005069656919554
  %param_dsel_p_89 = select i1 %param_unset_p_89, i64 0, i64 %arg0
  store i64 %param_dsel_p_89, i64* %param_p_90
  %param_unset_v_91 = icmp eq i64 %arg1, -581005069656919554
  %param_dsel_v_91 = select i1 %param_unset_v_91, i64 0, i64 %arg1
  store i64 %param_dsel_v_91, i64* %param_v_92
  %t64_93 = load i64, i64* %param_p_90
  %t64_94 = load i64, i64* %param_v_92
  %t_95 = ashr i64 %t64_94, 24
  %t_96 = and i64 %t_95, 255
  %fn_ret_97 = call i64 @__omni_cyw_priv_w8(i64 %t64_93, i64 %t_96, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_98 = load i64, i64* %param_p_90
  %t_99 = add i64 %t64_98, 1
  %t64_100 = load i64, i64* %param_v_92
  %t_101 = ashr i64 %t64_100, 16
  %t_102 = and i64 %t_101, 255
  %fn_ret_103 = call i64 @__omni_cyw_priv_w8(i64 %t_99, i64 %t_102, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_104 = load i64, i64* %param_p_90
  %t_105 = add i64 %t64_104, 2
  %t64_106 = load i64, i64* %param_v_92
  %t_107 = ashr i64 %t64_106, 8
  %t_108 = and i64 %t_107, 255
  %fn_ret_109 = call i64 @__omni_cyw_priv_w8(i64 %t_105, i64 %t_108, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_110 = load i64, i64* %param_p_90
  %t_111 = add i64 %t64_110, 3
  %t64_112 = load i64, i64* %param_v_92
  %t_113 = and i64 %t64_112, 255
  %fn_ret_114 = call i64 @__omni_cyw_priv_w8(i64 %t_111, i64 %t_113, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  ret i64 0
}


define void @__omni_trampoline_w32_be(i8* %raw_arg) {
entry:
  %raw_int = ptrtoint i8* %raw_arg to i64
  %res = call i64 @__omni_cyw_priv_w32_be(i64 %raw_int, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  ret void
}

define void @__omni_trampoline_pack_w32_be(i8* %raw_pack) {
entry:
  %p64 = bitcast i8* %raw_pack to i64*
  %p0 = getelementptr inbounds i64, i64* %p64, i64 0
  %a0 = load i64, i64* %p0
  %p1 = getelementptr inbounds i64, i64* %p64, i64 1
  %a1 = load i64, i64* %p1
  %p2 = getelementptr inbounds i64, i64* %p64, i64 2
  %a2 = load i64, i64* %p2
  %p3 = getelementptr inbounds i64, i64* %p64, i64 3
  %a3 = load i64, i64* %p3
  %p4 = getelementptr inbounds i64, i64* %p64, i64 4
  %a4 = load i64, i64* %p4
  %p5 = getelementptr inbounds i64, i64* %p64, i64 5
  %a5 = load i64, i64* %p5
  call void @free(i8* %raw_pack)
  %res = call i64 @__omni_cyw_priv_w32_be(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5)
  ret void
}

define i64 @__omni_cyw_priv_r32_le(i64 %arg0, i64 %arg1, i64 %arg2, i64 %arg3, i64 %arg4, i64 %arg5) {
entry:
  %param_p_116 = alloca i64
  %var_b0_119 = alloca i64
  %var_b1_124 = alloca i64
  %var_b2_129 = alloca i64
  %var_b3_134 = alloca i64
  %param_unset_p_115 = icmp eq i64 %arg0, -581005069656919554
  %param_dsel_p_115 = select i1 %param_unset_p_115, i64 0, i64 %arg0
  store i64 %param_dsel_p_115, i64* %param_p_116
  call void @__omni_c_return_ptr(i8* null)
  %t64_117 = load i64, i64* %param_p_116
  %fn_ret_118 = call i64 @__omni_cyw_priv_r8(i64 %t64_117, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  store i64 %fn_ret_118, i64* %var_b0_119
  call void @__omni_c_return_ptr(i8* null)
  %t64_120 = load i64, i64* %param_p_116
  %t_121 = add i64 %t64_120, 1
  %fn_ret_122 = call i64 @__omni_cyw_priv_r8(i64 %t_121, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t_123 = shl i64 %fn_ret_122, 8
  store i64 %t_123, i64* %var_b1_124
  call void @__omni_c_return_ptr(i8* null)
  %t64_125 = load i64, i64* %param_p_116
  %t_126 = add i64 %t64_125, 2
  %fn_ret_127 = call i64 @__omni_cyw_priv_r8(i64 %t_126, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t_128 = shl i64 %fn_ret_127, 16
  store i64 %t_128, i64* %var_b2_129
  call void @__omni_c_return_ptr(i8* null)
  %t64_130 = load i64, i64* %param_p_116
  %t_131 = add i64 %t64_130, 3
  %fn_ret_132 = call i64 @__omni_cyw_priv_r8(i64 %t_131, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t_133 = shl i64 %fn_ret_132, 24
  store i64 %t_133, i64* %var_b3_134
  %t64_135 = load i64, i64* %var_b0_119
  %t64_136 = load i64, i64* %var_b1_124
  %t_137 = or i64 %t64_135, %t64_136
  %t64_138 = load i64, i64* %var_b2_129
  %t_139 = or i64 %t_137, %t64_138
  %t64_140 = load i64, i64* %var_b3_134
  %t_141 = or i64 %t_139, %t64_140
  %t_142 = and i64 %t_141, 4294967295
  ret i64 %t_142
}


define void @__omni_trampoline_r32_le(i8* %raw_arg) {
entry:
  %raw_int = ptrtoint i8* %raw_arg to i64
  %res = call i64 @__omni_cyw_priv_r32_le(i64 %raw_int, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  ret void
}

define void @__omni_trampoline_pack_r32_le(i8* %raw_pack) {
entry:
  %p64 = bitcast i8* %raw_pack to i64*
  %p0 = getelementptr inbounds i64, i64* %p64, i64 0
  %a0 = load i64, i64* %p0
  %p1 = getelementptr inbounds i64, i64* %p64, i64 1
  %a1 = load i64, i64* %p1
  %p2 = getelementptr inbounds i64, i64* %p64, i64 2
  %a2 = load i64, i64* %p2
  %p3 = getelementptr inbounds i64, i64* %p64, i64 3
  %a3 = load i64, i64* %p3
  %p4 = getelementptr inbounds i64, i64* %p64, i64 4
  %a4 = load i64, i64* %p4
  %p5 = getelementptr inbounds i64, i64* %p64, i64 5
  %a5 = load i64, i64* %p5
  call void @free(i8* %raw_pack)
  %res = call i64 @__omni_cyw_priv_r32_le(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5)
  ret void
}

define i64 @__omni_cyw_priv_w32_le(i64 %arg0, i64 %arg1, i64 %arg2, i64 %arg3, i64 %arg4, i64 %arg5) {
entry:
  %param_p_144 = alloca i64
  %param_v_146 = alloca i64
  %param_unset_p_143 = icmp eq i64 %arg0, -581005069656919554
  %param_dsel_p_143 = select i1 %param_unset_p_143, i64 0, i64 %arg0
  store i64 %param_dsel_p_143, i64* %param_p_144
  %param_unset_v_145 = icmp eq i64 %arg1, -581005069656919554
  %param_dsel_v_145 = select i1 %param_unset_v_145, i64 0, i64 %arg1
  store i64 %param_dsel_v_145, i64* %param_v_146
  %t64_147 = load i64, i64* %param_p_144
  %t64_148 = load i64, i64* %param_v_146
  %t_149 = and i64 %t64_148, 255
  %fn_ret_150 = call i64 @__omni_cyw_priv_w8(i64 %t64_147, i64 %t_149, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_151 = load i64, i64* %param_p_144
  %t_152 = add i64 %t64_151, 1
  %t64_153 = load i64, i64* %param_v_146
  %t_154 = ashr i64 %t64_153, 8
  %t_155 = and i64 %t_154, 255
  %fn_ret_156 = call i64 @__omni_cyw_priv_w8(i64 %t_152, i64 %t_155, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_157 = load i64, i64* %param_p_144
  %t_158 = add i64 %t64_157, 2
  %t64_159 = load i64, i64* %param_v_146
  %t_160 = ashr i64 %t64_159, 16
  %t_161 = and i64 %t_160, 255
  %fn_ret_162 = call i64 @__omni_cyw_priv_w8(i64 %t_158, i64 %t_161, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_163 = load i64, i64* %param_p_144
  %t_164 = add i64 %t64_163, 3
  %t64_165 = load i64, i64* %param_v_146
  %t_166 = ashr i64 %t64_165, 24
  %t_167 = and i64 %t_166, 255
  %fn_ret_168 = call i64 @__omni_cyw_priv_w8(i64 %t_164, i64 %t_167, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  ret i64 0
}


define void @__omni_trampoline_w32_le(i8* %raw_arg) {
entry:
  %raw_int = ptrtoint i8* %raw_arg to i64
  %res = call i64 @__omni_cyw_priv_w32_le(i64 %raw_int, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  ret void
}

define void @__omni_trampoline_pack_w32_le(i8* %raw_pack) {
entry:
  %p64 = bitcast i8* %raw_pack to i64*
  %p0 = getelementptr inbounds i64, i64* %p64, i64 0
  %a0 = load i64, i64* %p0
  %p1 = getelementptr inbounds i64, i64* %p64, i64 1
  %a1 = load i64, i64* %p1
  %p2 = getelementptr inbounds i64, i64* %p64, i64 2
  %a2 = load i64, i64* %p2
  %p3 = getelementptr inbounds i64, i64* %p64, i64 3
  %a3 = load i64, i64* %p3
  %p4 = getelementptr inbounds i64, i64* %p64, i64 4
  %a4 = load i64, i64* %p4
  %p5 = getelementptr inbounds i64, i64* %p64, i64 5
  %a5 = load i64, i64* %p5
  call void @free(i8* %raw_pack)
  %res = call i64 @__omni_cyw_priv_w32_le(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5)
  ret void
}

define i64 @__omni_cyw_priv_bzero(i64 %arg0, i64 %arg1, i64 %arg2, i64 %arg3, i64 %arg4, i64 %arg5) {
entry:
  %param_p_170 = alloca i64
  %param_n_172 = alloca i64
  %var_i_173 = alloca i64
  %param_unset_p_169 = icmp eq i64 %arg0, -581005069656919554
  %param_dsel_p_169 = select i1 %param_unset_p_169, i64 0, i64 %arg0
  store i64 %param_dsel_p_169, i64* %param_p_170
  %param_unset_n_171 = icmp eq i64 %arg1, -581005069656919554
  %param_dsel_n_171 = select i1 %param_unset_n_171, i64 0, i64 %arg1
  store i64 %param_dsel_n_171, i64* %param_n_172
  call void @__omni_c_return_ptr(i8* null)
  store i64 0, i64* %var_i_173
  br label %while_cond_0

while_cond_0:
  %t64_174 = load i64, i64* %var_i_173
  %t64_175 = load i64, i64* %param_n_172
  %cond_176 = icmp slt i64 %t64_174, %t64_175
  br i1 %cond_176, label %while_body_0, label %while_exit_0

while_body_0:
  %t64_177 = load i64, i64* %param_p_170
  %t64_178 = load i64, i64* %var_i_173
  %t_179 = add i64 %t64_177, %t64_178
  %fn_ret_180 = call i64 @__omni_cyw_priv_w8(i64 %t_179, i64 0, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  call void @__omni_c_return_ptr(i8* null)
  %t64_181 = load i64, i64* %var_i_173
  %t_182 = add i64 %t64_181, 1
  store i64 %t_182, i64* %var_i_173
  br label %while_cond_0

while_exit_0:
  ret i64 0
}


define void @__omni_trampoline_bzero(i8* %raw_arg) {
entry:
  %raw_int = ptrtoint i8* %raw_arg to i64
  %res = call i64 @__omni_cyw_priv_bzero(i64 %raw_int, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  ret void
}

define void @__omni_trampoline_pack_bzero(i8* %raw_pack) {
entry:
  %p64 = bitcast i8* %raw_pack to i64*
  %p0 = getelementptr inbounds i64, i64* %p64, i64 0
  %a0 = load i64, i64* %p0
  %p1 = getelementptr inbounds i64, i64* %p64, i64 1
  %a1 = load i64, i64* %p1
  %p2 = getelementptr inbounds i64, i64* %p64, i64 2
  %a2 = load i64, i64* %p2
  %p3 = getelementptr inbounds i64, i64* %p64, i64 3
  %a3 = load i64, i64* %p3
  %p4 = getelementptr inbounds i64, i64* %p64, i64 4
  %a4 = load i64, i64* %p4
  %p5 = getelementptr inbounds i64, i64* %p64, i64 5
  %a5 = load i64, i64* %p5
  call void @free(i8* %raw_pack)
  %res = call i64 @__omni_cyw_priv_bzero(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5)
  ret void
}

define i64 @__omni_cyw_priv_bcopy(i64 %arg0, i64 %arg1, i64 %arg2, i64 %arg3, i64 %arg4, i64 %arg5) {
entry:
  %param_dst_184 = alloca i64
  %param_src_186 = alloca i64
  %param_n_188 = alloca i64
  %var_i_189 = alloca i64
  %param_unset_dst_183 = icmp eq i64 %arg0, -581005069656919554
  %param_dsel_dst_183 = select i1 %param_unset_dst_183, i64 0, i64 %arg0
  store i64 %param_dsel_dst_183, i64* %param_dst_184
  %param_unset_src_185 = icmp eq i64 %arg1, -581005069656919554
  %param_dsel_src_185 = select i1 %param_unset_src_185, i64 0, i64 %arg1
  store i64 %param_dsel_src_185, i64* %param_src_186
  %param_unset_n_187 = icmp eq i64 %arg2, -581005069656919554
  %param_dsel_n_187 = select i1 %param_unset_n_187, i64 0, i64 %arg2
  store i64 %param_dsel_n_187, i64* %param_n_188
  call void @__omni_c_return_ptr(i8* null)
  store i64 0, i64* %var_i_189
  br label %while_cond_1

while_cond_1:
  %t64_190 = load i64, i64* %var_i_189
  %t64_191 = load i64, i64* %param_n_188
  %cond_192 = icmp slt i64 %t64_190, %t64_191
  br i1 %cond_192, label %while_body_1, label %while_exit_1

while_body_1:
  %t64_193 = load i64, i64* %param_dst_184
  %t64_194 = load i64, i64* %var_i_189
  %t_195 = add i64 %t64_193, %t64_194
  %t64_196 = load i64, i64* %param_src_186
  %t64_197 = load i64, i64* %var_i_189
  %t_198 = add i64 %t64_196, %t64_197
  %fn_ret_199 = call i64 @__omni_cyw_priv_r8(i64 %t_198, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %fn_ret_200 = call i64 @__omni_cyw_priv_w8(i64 %t_195, i64 %fn_ret_199, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  call void @__omni_c_return_ptr(i8* null)
  %t64_201 = load i64, i64* %var_i_189
  %t_202 = add i64 %t64_201, 1
  store i64 %t_202, i64* %var_i_189
  br label %while_cond_1

while_exit_1:
  %t64_203 = load i64, i64* %param_n_188
  ret i64 %t64_203
}


define void @__omni_trampoline_bcopy(i8* %raw_arg) {
entry:
  %raw_int = ptrtoint i8* %raw_arg to i64
  %res = call i64 @__omni_cyw_priv_bcopy(i64 %raw_int, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  ret void
}

define void @__omni_trampoline_pack_bcopy(i8* %raw_pack) {
entry:
  %p64 = bitcast i8* %raw_pack to i64*
  %p0 = getelementptr inbounds i64, i64* %p64, i64 0
  %a0 = load i64, i64* %p0
  %p1 = getelementptr inbounds i64, i64* %p64, i64 1
  %a1 = load i64, i64* %p1
  %p2 = getelementptr inbounds i64, i64* %p64, i64 2
  %a2 = load i64, i64* %p2
  %p3 = getelementptr inbounds i64, i64* %p64, i64 3
  %a3 = load i64, i64* %p3
  %p4 = getelementptr inbounds i64, i64* %p64, i64 4
  %a4 = load i64, i64* %p4
  %p5 = getelementptr inbounds i64, i64* %p64, i64 5
  %a5 = load i64, i64* %p5
  call void @free(i8* %raw_pack)
  %res = call i64 @__omni_cyw_priv_bcopy(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5)
  ret void
}

define i64 @__omni_cyw_priv_cstr_len(i64 %arg0, i64 %arg1, i64 %arg2, i64 %arg3, i64 %arg4, i64 %arg5) {
entry:
  %param_p_205 = alloca i64
  %var_n_208 = alloca i64
  %param_unset_p_204 = icmp eq i64 %arg0, -581005069656919554
  %param_dsel_p_204 = select i1 %param_unset_p_204, i64 0, i64 %arg0
  store i64 %param_dsel_p_204, i64* %param_p_205
  %t64_206 = load i64, i64* %param_p_205
  %cond_207 = icmp eq i64 %t64_206, 0
  br i1 %cond_207, label %if_then_2, label %if_merge_2

if_then_2:
  ret i64 0
if_merge_2:
  call void @__omni_c_return_ptr(i8* null)
  store i64 0, i64* %var_n_208
  br label %while_cond_3

while_cond_3:
  %t64_209 = load i64, i64* %var_n_208
  %cond_210 = icmp slt i64 %t64_209, 1024
  br i1 %cond_210, label %while_body_3, label %while_exit_3

while_body_3:
  %t64_211 = load i64, i64* %param_p_205
  %t64_212 = load i64, i64* %var_n_208
  %t_213 = add i64 %t64_211, %t64_212
  %fn_ret_214 = call i64 @__omni_cyw_priv_r8(i64 %t_213, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %cond_215 = icmp eq i64 %fn_ret_214, 0
  br i1 %cond_215, label %if_then_4, label %if_merge_4

if_then_4:
  %t64_216 = load i64, i64* %var_n_208
  ret i64 %t64_216
if_merge_4:
  call void @__omni_c_return_ptr(i8* null)
  %t64_217 = load i64, i64* %var_n_208
  %t_218 = add i64 %t64_217, 1
  store i64 %t_218, i64* %var_n_208
  br label %while_cond_3

while_exit_3:
  %t64_219 = load i64, i64* %var_n_208
  ret i64 %t64_219
}


define void @__omni_trampoline_cstr_len(i8* %raw_arg) {
entry:
  %raw_int = ptrtoint i8* %raw_arg to i64
  %res = call i64 @__omni_cyw_priv_cstr_len(i64 %raw_int, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  ret void
}

define void @__omni_trampoline_pack_cstr_len(i8* %raw_pack) {
entry:
  %p64 = bitcast i8* %raw_pack to i64*
  %p0 = getelementptr inbounds i64, i64* %p64, i64 0
  %a0 = load i64, i64* %p0
  %p1 = getelementptr inbounds i64, i64* %p64, i64 1
  %a1 = load i64, i64* %p1
  %p2 = getelementptr inbounds i64, i64* %p64, i64 2
  %a2 = load i64, i64* %p2
  %p3 = getelementptr inbounds i64, i64* %p64, i64 3
  %a3 = load i64, i64* %p3
  %p4 = getelementptr inbounds i64, i64* %p64, i64 4
  %a4 = load i64, i64* %p4
  %p5 = getelementptr inbounds i64, i64* %p64, i64 5
  %a5 = load i64, i64* %p5
  call void @free(i8* %raw_pack)
  %res = call i64 @__omni_cyw_priv_cstr_len(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5)
  ret void
}

define i64 @__omni_cyw_priv_put_ascii(i64 %arg0, i64 %arg1, i64 %arg2, i64 %arg3, i64 %arg4, i64 %arg5) {
entry:
  %param_dst_221 = alloca i64
  %param_s_223 = alloca i64
  %var_n_226 = alloca i64
  %param_unset_dst_220 = icmp eq i64 %arg0, -581005069656919554
  %param_dsel_dst_220 = select i1 %param_unset_dst_220, i64 0, i64 %arg0
  store i64 %param_dsel_dst_220, i64* %param_dst_221
  %param_unset_s_222 = icmp eq i64 %arg1, -581005069656919554
  %param_dsel_s_222 = select i1 %param_unset_s_222, i64 0, i64 %arg1
  store i64 %param_dsel_s_222, i64* %param_s_223
  call void @__omni_c_return_ptr(i8* null)
  %t64_224 = load i64, i64* %param_s_223
  %fn_ret_225 = call i64 @__omni_cyw_priv_cstr_len(i64 %t64_224, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  store i64 %fn_ret_225, i64* %var_n_226
  %t64_227 = load i64, i64* %param_dst_221
  %t64_228 = load i64, i64* %param_s_223
  %t64_229 = load i64, i64* %var_n_226
  %fn_ret_230 = call i64 @__omni_cyw_priv_bcopy(i64 %t64_227, i64 %t64_228, i64 %t64_229, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_231 = load i64, i64* %var_n_226
  ret i64 %t64_231
}


define void @__omni_trampoline_put_ascii(i8* %raw_arg) {
entry:
  %raw_int = ptrtoint i8* %raw_arg to i64
  %res = call i64 @__omni_cyw_priv_put_ascii(i64 %raw_int, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  ret void
}

define void @__omni_trampoline_pack_put_ascii(i8* %raw_pack) {
entry:
  %p64 = bitcast i8* %raw_pack to i64*
  %p0 = getelementptr inbounds i64, i64* %p64, i64 0
  %a0 = load i64, i64* %p0
  %p1 = getelementptr inbounds i64, i64* %p64, i64 1
  %a1 = load i64, i64* %p1
  %p2 = getelementptr inbounds i64, i64* %p64, i64 2
  %a2 = load i64, i64* %p2
  %p3 = getelementptr inbounds i64, i64* %p64, i64 3
  %a3 = load i64, i64* %p3
  %p4 = getelementptr inbounds i64, i64* %p64, i64 4
  %a4 = load i64, i64* %p4
  %p5 = getelementptr inbounds i64, i64* %p64, i64 5
  %a5 = load i64, i64* %p5
  call void @free(i8* %raw_pack)
  %res = call i64 @__omni_cyw_priv_put_ascii(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5)
  ret void
}

define i64 @__omni_cyw_priv_now_ms(i64 %arg0, i64 %arg1, i64 %arg2, i64 %arg3, i64 %arg4, i64 %arg5) {
entry:
  %var_us_234 = alloca i64
  call void @__omni_c_return_ptr(i8* null)
  %ext_i64_232 = call i64 @time_us()
  %t_233 = and i64 %ext_i64_232, 4294967295
  store i64 %t_233, i64* %var_us_234
  %t64_235 = load i64, i64* %var_us_234
  %div_zero_237 = icmp eq i64 1000, 0
  %safe_div_237 = select i1 %div_zero_237, i64 1, i64 1000
  %t_237 = sdiv i64 %t64_235, %safe_div_237
  %t_238 = and i64 %t_237, 2147483647
  ret i64 %t_238
}


define void @__omni_trampoline_now_ms(i8* %raw_arg) {
entry:
  %raw_int = ptrtoint i8* %raw_arg to i64
  %res = call i64 @__omni_cyw_priv_now_ms(i64 %raw_int, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  ret void
}

define void @__omni_trampoline_pack_now_ms(i8* %raw_pack) {
entry:
  %p64 = bitcast i8* %raw_pack to i64*
  %p0 = getelementptr inbounds i64, i64* %p64, i64 0
  %a0 = load i64, i64* %p0
  %p1 = getelementptr inbounds i64, i64* %p64, i64 1
  %a1 = load i64, i64* %p1
  %p2 = getelementptr inbounds i64, i64* %p64, i64 2
  %a2 = load i64, i64* %p2
  %p3 = getelementptr inbounds i64, i64* %p64, i64 3
  %a3 = load i64, i64* %p3
  %p4 = getelementptr inbounds i64, i64* %p64, i64 4
  %a4 = load i64, i64* %p4
  %p5 = getelementptr inbounds i64, i64* %p64, i64 5
  %a5 = load i64, i64* %p5
  call void @free(i8* %raw_pack)
  %res = call i64 @__omni_cyw_priv_now_ms(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5)
  ret void
}

define i64 @__omni_cyw_priv_get_net_ctx(i64 %arg0, i64 %arg1, i64 %arg2, i64 %arg3, i64 %arg4, i64 %arg5) {
entry:
  %var_scr_240 = alloca i64
  %var_ptr_244 = alloca i64
  %var_mem_249 = alloca i64
  call void @__omni_c_return_ptr(i8* null)
  %ext_i64_239 = call i64 @cyw_ll_sram_scratch()
  store i64 %ext_i64_239, i64* %var_scr_240
  call void @__omni_c_return_ptr(i8* null)
  %t64_241 = load i64, i64* %var_scr_240
  %t_242 = add i64 %t64_241, 16
  %ext_i64_243 = call i64 @mmio_read32(i64 %t_242)
  store i64 %ext_i64_243, i64* %var_ptr_244
  %t64_245 = load i64, i64* %var_ptr_244
  %cond_246 = icmp ne i64 %t64_245, 0
  br i1 %cond_246, label %if_then_5, label %if_merge_5

if_then_5:
  %t64_247 = load i64, i64* %var_ptr_244
  ret i64 %t64_247
if_merge_5:
  call void @__omni_c_return_ptr(i8* null)
  %fn_ret_248 = call i64 @calloc(i64 1, i64 9216)
  store i64 %fn_ret_248, i64* %var_mem_249
  %t64_250 = load i64, i64* %var_scr_240
  %t_251 = add i64 %t64_250, 16
  %t64_252 = load i64, i64* %var_mem_249
  %ext_i64_253 = call i64 @mmio_write32(i64 %t_251, i64 %t64_252)
  %t64_254 = load i64, i64* %var_mem_249
  %t_255 = add i64 %t64_254, 16
  %ext_i64_256 = call i64 @mmio_write32(i64 %t_255, i64 1424)
  %t64_257 = load i64, i64* %var_mem_249
  %t_258 = add i64 %t64_257, 20
  %ext_i64_259 = call i64 @mmio_write32(i64 %t_258, i64 1)
  %t64_260 = load i64, i64* %var_mem_249
  %t_261 = add i64 %t64_260, 24
  %ext_i64_262 = call i64 @mmio_write32(i64 %t_261, i64 1330466377)
  %t64_263 = load i64, i64* %var_mem_249
  %t_264 = add i64 %t64_263, 128
  %fn_ret_265 = call i64 @__omni_cyw_priv_w8(i64 %t_264, i64 48, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_266 = load i64, i64* %var_mem_249
  %t_267 = add i64 %t64_266, 129
  %fn_ret_268 = call i64 @__omni_cyw_priv_w8(i64 %t_267, i64 46, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_269 = load i64, i64* %var_mem_249
  %t_270 = add i64 %t64_269, 130
  %fn_ret_271 = call i64 @__omni_cyw_priv_w8(i64 %t_270, i64 48, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_272 = load i64, i64* %var_mem_249
  %t_273 = add i64 %t64_272, 131
  %fn_ret_274 = call i64 @__omni_cyw_priv_w8(i64 %t_273, i64 46, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_275 = load i64, i64* %var_mem_249
  %t_276 = add i64 %t64_275, 132
  %fn_ret_277 = call i64 @__omni_cyw_priv_w8(i64 %t_276, i64 48, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_278 = load i64, i64* %var_mem_249
  %t_279 = add i64 %t64_278, 133
  %fn_ret_280 = call i64 @__omni_cyw_priv_w8(i64 %t_279, i64 46, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_281 = load i64, i64* %var_mem_249
  %t_282 = add i64 %t64_281, 134
  %fn_ret_283 = call i64 @__omni_cyw_priv_w8(i64 %t_282, i64 48, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_284 = load i64, i64* %var_mem_249
  %t_285 = add i64 %t64_284, 135
  %fn_ret_286 = call i64 @__omni_cyw_priv_w8(i64 %t_285, i64 0, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_287 = load i64, i64* %var_mem_249
  ret i64 %t64_287
}


define void @__omni_trampoline_get_net_ctx(i8* %raw_arg) {
entry:
  %raw_int = ptrtoint i8* %raw_arg to i64
  %res = call i64 @__omni_cyw_priv_get_net_ctx(i64 %raw_int, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  ret void
}

define void @__omni_trampoline_pack_get_net_ctx(i8* %raw_pack) {
entry:
  %p64 = bitcast i8* %raw_pack to i64*
  %p0 = getelementptr inbounds i64, i64* %p64, i64 0
  %a0 = load i64, i64* %p0
  %p1 = getelementptr inbounds i64, i64* %p64, i64 1
  %a1 = load i64, i64* %p1
  %p2 = getelementptr inbounds i64, i64* %p64, i64 2
  %a2 = load i64, i64* %p2
  %p3 = getelementptr inbounds i64, i64* %p64, i64 3
  %a3 = load i64, i64* %p3
  %p4 = getelementptr inbounds i64, i64* %p64, i64 4
  %a4 = load i64, i64* %p4
  %p5 = getelementptr inbounds i64, i64* %p64, i64 5
  %a5 = load i64, i64* %p5
  call void @free(i8* %raw_pack)
  %res = call i64 @__omni_cyw_priv_get_net_ctx(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5)
  ret void
}

define i64 @__omni_cyw_priv_mac_eq(i64 %arg0, i64 %arg1, i64 %arg2, i64 %arg3, i64 %arg4, i64 %arg5) {
entry:
  %param_a_289 = alloca i64
  %param_b_291 = alloca i64
  %var_i_292 = alloca i64
  %param_unset_a_288 = icmp eq i64 %arg0, -581005069656919554
  %param_dsel_a_288 = select i1 %param_unset_a_288, i64 0, i64 %arg0
  store i64 %param_dsel_a_288, i64* %param_a_289
  %param_unset_b_290 = icmp eq i64 %arg1, -581005069656919554
  %param_dsel_b_290 = select i1 %param_unset_b_290, i64 0, i64 %arg1
  store i64 %param_dsel_b_290, i64* %param_b_291
  call void @__omni_c_return_ptr(i8* null)
  store i64 0, i64* %var_i_292
  br label %while_cond_6

while_cond_6:
  %t64_293 = load i64, i64* %var_i_292
  %cond_294 = icmp slt i64 %t64_293, 6
  br i1 %cond_294, label %while_body_6, label %while_exit_6

while_body_6:
  %t64_295 = load i64, i64* %param_a_289
  %t64_296 = load i64, i64* %var_i_292
  %t_297 = add i64 %t64_295, %t64_296
  %fn_ret_298 = call i64 @__omni_cyw_priv_r8(i64 %t_297, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_299 = load i64, i64* %param_b_291
  %t64_300 = load i64, i64* %var_i_292
  %t_301 = add i64 %t64_299, %t64_300
  %fn_ret_302 = call i64 @__omni_cyw_priv_r8(i64 %t_301, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %cond_303 = icmp ne i64 %fn_ret_298, %fn_ret_302
  br i1 %cond_303, label %if_then_7, label %if_merge_7

if_then_7:
  ret i64 0
if_merge_7:
  call void @__omni_c_return_ptr(i8* null)
  %t64_304 = load i64, i64* %var_i_292
  %t_305 = add i64 %t64_304, 1
  store i64 %t_305, i64* %var_i_292
  br label %while_cond_6

while_exit_6:
  ret i64 1
}


define void @__omni_trampoline_mac_eq(i8* %raw_arg) {
entry:
  %raw_int = ptrtoint i8* %raw_arg to i64
  %res = call i64 @__omni_cyw_priv_mac_eq(i64 %raw_int, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  ret void
}

define void @__omni_trampoline_pack_mac_eq(i8* %raw_pack) {
entry:
  %p64 = bitcast i8* %raw_pack to i64*
  %p0 = getelementptr inbounds i64, i64* %p64, i64 0
  %a0 = load i64, i64* %p0
  %p1 = getelementptr inbounds i64, i64* %p64, i64 1
  %a1 = load i64, i64* %p1
  %p2 = getelementptr inbounds i64, i64* %p64, i64 2
  %a2 = load i64, i64* %p2
  %p3 = getelementptr inbounds i64, i64* %p64, i64 3
  %a3 = load i64, i64* %p3
  %p4 = getelementptr inbounds i64, i64* %p64, i64 4
  %a4 = load i64, i64* %p4
  %p5 = getelementptr inbounds i64, i64* %p64, i64 5
  %a5 = load i64, i64* %p5
  call void @free(i8* %raw_pack)
  %res = call i64 @__omni_cyw_priv_mac_eq(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5)
  ret void
}

define i64 @__omni_cyw_priv_mac_is_bcast(i64 %arg0, i64 %arg1, i64 %arg2, i64 %arg3, i64 %arg4, i64 %arg5) {
entry:
  %param_a_307 = alloca i64
  %var_m_330 = alloca i64
  %param_unset_a_306 = icmp eq i64 %arg0, -581005069656919554
  %param_dsel_a_306 = select i1 %param_unset_a_306, i64 0, i64 %arg0
  store i64 %param_dsel_a_306, i64* %param_a_307
  call void @__omni_c_return_ptr(i8* null)
  %t64_308 = load i64, i64* %param_a_307
  %fn_ret_309 = call i64 @__omni_cyw_priv_r8(i64 %t64_308, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_310 = load i64, i64* %param_a_307
  %t_311 = add i64 %t64_310, 1
  %fn_ret_312 = call i64 @__omni_cyw_priv_r8(i64 %t_311, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t_313 = and i64 %fn_ret_309, %fn_ret_312
  %t64_314 = load i64, i64* %param_a_307
  %t_315 = add i64 %t64_314, 2
  %fn_ret_316 = call i64 @__omni_cyw_priv_r8(i64 %t_315, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t_317 = and i64 %t_313, %fn_ret_316
  %t64_318 = load i64, i64* %param_a_307
  %t_319 = add i64 %t64_318, 3
  %fn_ret_320 = call i64 @__omni_cyw_priv_r8(i64 %t_319, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t_321 = and i64 %t_317, %fn_ret_320
  %t64_322 = load i64, i64* %param_a_307
  %t_323 = add i64 %t64_322, 4
  %fn_ret_324 = call i64 @__omni_cyw_priv_r8(i64 %t_323, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t_325 = and i64 %t_321, %fn_ret_324
  %t64_326 = load i64, i64* %param_a_307
  %t_327 = add i64 %t64_326, 5
  %fn_ret_328 = call i64 @__omni_cyw_priv_r8(i64 %t_327, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t_329 = and i64 %t_325, %fn_ret_328
  store i64 %t_329, i64* %var_m_330
  %t64_331 = load i64, i64* %var_m_330
  %cond_332 = icmp eq i64 %t64_331, 255
  br i1 %cond_332, label %if_then_8, label %if_merge_8

if_then_8:
  ret i64 1
if_merge_8:
  ret i64 0
}


define void @__omni_trampoline_mac_is_bcast(i8* %raw_arg) {
entry:
  %raw_int = ptrtoint i8* %raw_arg to i64
  %res = call i64 @__omni_cyw_priv_mac_is_bcast(i64 %raw_int, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  ret void
}

define void @__omni_trampoline_pack_mac_is_bcast(i8* %raw_pack) {
entry:
  %p64 = bitcast i8* %raw_pack to i64*
  %p0 = getelementptr inbounds i64, i64* %p64, i64 0
  %a0 = load i64, i64* %p0
  %p1 = getelementptr inbounds i64, i64* %p64, i64 1
  %a1 = load i64, i64* %p1
  %p2 = getelementptr inbounds i64, i64* %p64, i64 2
  %a2 = load i64, i64* %p2
  %p3 = getelementptr inbounds i64, i64* %p64, i64 3
  %a3 = load i64, i64* %p3
  %p4 = getelementptr inbounds i64, i64* %p64, i64 4
  %a4 = load i64, i64* %p4
  %p5 = getelementptr inbounds i64, i64* %p64, i64 5
  %a5 = load i64, i64* %p5
  call void @free(i8* %raw_pack)
  %res = call i64 @__omni_cyw_priv_mac_is_bcast(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5)
  ret void
}

define i64 @__omni_cyw_priv_ip_eq(i64 %arg0, i64 %arg1, i64 %arg2, i64 %arg3, i64 %arg4, i64 %arg5) {
entry:
  %param_a_334 = alloca i64
  %param_b_336 = alloca i64
  %param_unset_a_333 = icmp eq i64 %arg0, -581005069656919554
  %param_dsel_a_333 = select i1 %param_unset_a_333, i64 0, i64 %arg0
  store i64 %param_dsel_a_333, i64* %param_a_334
  %param_unset_b_335 = icmp eq i64 %arg1, -581005069656919554
  %param_dsel_b_335 = select i1 %param_unset_b_335, i64 0, i64 %arg1
  store i64 %param_dsel_b_335, i64* %param_b_336
  %t64_340 = load i64, i64* %param_a_334
  %fn_ret_341 = call i64 @__omni_cyw_priv_r8(i64 %t64_340, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_342 = load i64, i64* %param_b_336
  %fn_ret_343 = call i64 @__omni_cyw_priv_r8(i64 %t64_342, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %cond_344 = icmp eq i64 %fn_ret_341, %fn_ret_343
  br i1 %cond_344, label %sc_and_rhs_339, label %sc_and_short_339

sc_and_short_339:
  br label %sc_and_merge_339

sc_and_rhs_339:
  %t64_345 = load i64, i64* %param_a_334
  %t_346 = add i64 %t64_345, 1
  %fn_ret_347 = call i64 @__omni_cyw_priv_r8(i64 %t_346, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_348 = load i64, i64* %param_b_336
  %t_349 = add i64 %t64_348, 1
  %fn_ret_350 = call i64 @__omni_cyw_priv_r8(i64 %t_349, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %cond_351 = icmp eq i64 %fn_ret_347, %fn_ret_350
  br label %sc_and_done_339

sc_and_done_339:
  br label %sc_and_merge_339

sc_and_merge_339:
  %and_cond_352 = phi i1 [ false, %sc_and_short_339 ], [ %cond_351, %sc_and_done_339 ]
  br i1 %and_cond_352, label %sc_and_rhs_338, label %sc_and_short_338

sc_and_short_338:
  br label %sc_and_merge_338

sc_and_rhs_338:
  %t64_353 = load i64, i64* %param_a_334
  %t_354 = add i64 %t64_353, 2
  %fn_ret_355 = call i64 @__omni_cyw_priv_r8(i64 %t_354, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_356 = load i64, i64* %param_b_336
  %t_357 = add i64 %t64_356, 2
  %fn_ret_358 = call i64 @__omni_cyw_priv_r8(i64 %t_357, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %cond_359 = icmp eq i64 %fn_ret_355, %fn_ret_358
  br label %sc_and_done_338

sc_and_done_338:
  br label %sc_and_merge_338

sc_and_merge_338:
  %and_cond_360 = phi i1 [ false, %sc_and_short_338 ], [ %cond_359, %sc_and_done_338 ]
  br i1 %and_cond_360, label %sc_and_rhs_337, label %sc_and_short_337

sc_and_short_337:
  br label %sc_and_merge_337

sc_and_rhs_337:
  %t64_361 = load i64, i64* %param_a_334
  %t_362 = add i64 %t64_361, 3
  %fn_ret_363 = call i64 @__omni_cyw_priv_r8(i64 %t_362, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_364 = load i64, i64* %param_b_336
  %t_365 = add i64 %t64_364, 3
  %fn_ret_366 = call i64 @__omni_cyw_priv_r8(i64 %t_365, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %cond_367 = icmp eq i64 %fn_ret_363, %fn_ret_366
  br label %sc_and_done_337

sc_and_done_337:
  br label %sc_and_merge_337

sc_and_merge_337:
  %and_cond_368 = phi i1 [ false, %sc_and_short_337 ], [ %cond_367, %sc_and_done_337 ]
  br i1 %and_cond_368, label %if_then_9, label %if_merge_9

if_then_9:
  ret i64 1
if_merge_9:
  ret i64 0
}


define void @__omni_trampoline_ip_eq(i8* %raw_arg) {
entry:
  %raw_int = ptrtoint i8* %raw_arg to i64
  %res = call i64 @__omni_cyw_priv_ip_eq(i64 %raw_int, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  ret void
}

define void @__omni_trampoline_pack_ip_eq(i8* %raw_pack) {
entry:
  %p64 = bitcast i8* %raw_pack to i64*
  %p0 = getelementptr inbounds i64, i64* %p64, i64 0
  %a0 = load i64, i64* %p0
  %p1 = getelementptr inbounds i64, i64* %p64, i64 1
  %a1 = load i64, i64* %p1
  %p2 = getelementptr inbounds i64, i64* %p64, i64 2
  %a2 = load i64, i64* %p2
  %p3 = getelementptr inbounds i64, i64* %p64, i64 3
  %a3 = load i64, i64* %p3
  %p4 = getelementptr inbounds i64, i64* %p64, i64 4
  %a4 = load i64, i64* %p4
  %p5 = getelementptr inbounds i64, i64* %p64, i64 5
  %a5 = load i64, i64* %p5
  call void @free(i8* %raw_pack)
  %res = call i64 @__omni_cyw_priv_ip_eq(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5)
  ret void
}

define i64 @__omni_cyw_priv_ip_is_bcast(i64 %arg0, i64 %arg1, i64 %arg2, i64 %arg3, i64 %arg4, i64 %arg5) {
entry:
  %param_a_370 = alloca i64
  %param_unset_a_369 = icmp eq i64 %arg0, -581005069656919554
  %param_dsel_a_369 = select i1 %param_unset_a_369, i64 0, i64 %arg0
  store i64 %param_dsel_a_369, i64* %param_a_370
  %t64_371 = load i64, i64* %param_a_370
  %fn_ret_372 = call i64 @__omni_cyw_priv_r8(i64 %t64_371, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_373 = load i64, i64* %param_a_370
  %t_374 = add i64 %t64_373, 1
  %fn_ret_375 = call i64 @__omni_cyw_priv_r8(i64 %t_374, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t_376 = and i64 %fn_ret_372, %fn_ret_375
  %t64_377 = load i64, i64* %param_a_370
  %t_378 = add i64 %t64_377, 2
  %fn_ret_379 = call i64 @__omni_cyw_priv_r8(i64 %t_378, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t_380 = and i64 %t_376, %fn_ret_379
  %t64_381 = load i64, i64* %param_a_370
  %t_382 = add i64 %t64_381, 3
  %fn_ret_383 = call i64 @__omni_cyw_priv_r8(i64 %t_382, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t_384 = and i64 %t_380, %fn_ret_383
  %cond_385 = icmp eq i64 %t_384, 255
  br i1 %cond_385, label %if_then_10, label %if_merge_10

if_then_10:
  ret i64 1
if_merge_10:
  ret i64 0
}


define void @__omni_trampoline_ip_is_bcast(i8* %raw_arg) {
entry:
  %raw_int = ptrtoint i8* %raw_arg to i64
  %res = call i64 @__omni_cyw_priv_ip_is_bcast(i64 %raw_int, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  ret void
}

define void @__omni_trampoline_pack_ip_is_bcast(i8* %raw_pack) {
entry:
  %p64 = bitcast i8* %raw_pack to i64*
  %p0 = getelementptr inbounds i64, i64* %p64, i64 0
  %a0 = load i64, i64* %p0
  %p1 = getelementptr inbounds i64, i64* %p64, i64 1
  %a1 = load i64, i64* %p1
  %p2 = getelementptr inbounds i64, i64* %p64, i64 2
  %a2 = load i64, i64* %p2
  %p3 = getelementptr inbounds i64, i64* %p64, i64 3
  %a3 = load i64, i64* %p3
  %p4 = getelementptr inbounds i64, i64* %p64, i64 4
  %a4 = load i64, i64* %p4
  %p5 = getelementptr inbounds i64, i64* %p64, i64 5
  %a5 = load i64, i64* %p5
  call void @free(i8* %raw_pack)
  %res = call i64 @__omni_cyw_priv_ip_is_bcast(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5)
  ret void
}

define i64 @__omni_cyw_priv_ip_is_zero(i64 %arg0, i64 %arg1, i64 %arg2, i64 %arg3, i64 %arg4, i64 %arg5) {
entry:
  %param_a_387 = alloca i64
  %param_unset_a_386 = icmp eq i64 %arg0, -581005069656919554
  %param_dsel_a_386 = select i1 %param_unset_a_386, i64 0, i64 %arg0
  store i64 %param_dsel_a_386, i64* %param_a_387
  %t64_388 = load i64, i64* %param_a_387
  %fn_ret_389 = call i64 @__omni_cyw_priv_r8(i64 %t64_388, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_390 = load i64, i64* %param_a_387
  %t_391 = add i64 %t64_390, 1
  %fn_ret_392 = call i64 @__omni_cyw_priv_r8(i64 %t_391, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t_393 = or i64 %fn_ret_389, %fn_ret_392
  %t64_394 = load i64, i64* %param_a_387
  %t_395 = add i64 %t64_394, 2
  %fn_ret_396 = call i64 @__omni_cyw_priv_r8(i64 %t_395, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t_397 = or i64 %t_393, %fn_ret_396
  %t64_398 = load i64, i64* %param_a_387
  %t_399 = add i64 %t64_398, 3
  %fn_ret_400 = call i64 @__omni_cyw_priv_r8(i64 %t_399, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t_401 = or i64 %t_397, %fn_ret_400
  %cond_402 = icmp eq i64 %t_401, 0
  br i1 %cond_402, label %if_then_11, label %if_merge_11

if_then_11:
  ret i64 1
if_merge_11:
  ret i64 0
}


define void @__omni_trampoline_ip_is_zero(i8* %raw_arg) {
entry:
  %raw_int = ptrtoint i8* %raw_arg to i64
  %res = call i64 @__omni_cyw_priv_ip_is_zero(i64 %raw_int, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  ret void
}

define void @__omni_trampoline_pack_ip_is_zero(i8* %raw_pack) {
entry:
  %p64 = bitcast i8* %raw_pack to i64*
  %p0 = getelementptr inbounds i64, i64* %p64, i64 0
  %a0 = load i64, i64* %p0
  %p1 = getelementptr inbounds i64, i64* %p64, i64 1
  %a1 = load i64, i64* %p1
  %p2 = getelementptr inbounds i64, i64* %p64, i64 2
  %a2 = load i64, i64* %p2
  %p3 = getelementptr inbounds i64, i64* %p64, i64 3
  %a3 = load i64, i64* %p3
  %p4 = getelementptr inbounds i64, i64* %p64, i64 4
  %a4 = load i64, i64* %p4
  %p5 = getelementptr inbounds i64, i64* %p64, i64 5
  %a5 = load i64, i64* %p5
  call void @free(i8* %raw_pack)
  %res = call i64 @__omni_cyw_priv_ip_is_zero(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5)
  ret void
}

define i64 @__omni_cyw_priv_write_u8_dec(i64 %arg0, i64 %arg1, i64 %arg2, i64 %arg3, i64 %arg4, i64 %arg5) {
entry:
  %param_dst_404 = alloca i64
  %param_val_406 = alloca i64
  %var_v_409 = alloca i64
  %var_pos_410 = alloca i64
  %var_d2_416 = alloca i64
  %var_rem_429 = alloca i64
  %var_d1_433 = alloca i64
  %var_d1_458 = alloca i64
  %param_unset_dst_403 = icmp eq i64 %arg0, -581005069656919554
  %param_dsel_dst_403 = select i1 %param_unset_dst_403, i64 0, i64 %arg0
  store i64 %param_dsel_dst_403, i64* %param_dst_404
  %param_unset_val_405 = icmp eq i64 %arg1, -581005069656919554
  %param_dsel_val_405 = select i1 %param_unset_val_405, i64 0, i64 %arg1
  store i64 %param_dsel_val_405, i64* %param_val_406
  call void @__omni_c_return_ptr(i8* null)
  %t64_407 = load i64, i64* %param_val_406
  %t_408 = and i64 %t64_407, 255
  store i64 %t_408, i64* %var_v_409
  call void @__omni_c_return_ptr(i8* null)
  store i64 0, i64* %var_pos_410
  %t64_411 = load i64, i64* %var_v_409
  %cond_412 = icmp sge i64 %t64_411, 100
  br i1 %cond_412, label %if_then_12, label %if_merge_12

if_then_12:
  call void @__omni_c_return_ptr(i8* null)
  %t64_413 = load i64, i64* %var_v_409
  %div_zero_415 = icmp eq i64 100, 0
  %safe_div_415 = select i1 %div_zero_415, i64 1, i64 100
  %t_415 = sdiv i64 %t64_413, %safe_div_415
  store i64 %t_415, i64* %var_d2_416
  %t64_417 = load i64, i64* %param_dst_404
  %t64_418 = load i64, i64* %var_pos_410
  %t_419 = add i64 %t64_417, %t64_418
  %t64_420 = load i64, i64* %var_d2_416
  %t_421 = add i64 48, %t64_420
  %fn_ret_422 = call i64 @__omni_cyw_priv_w8(i64 %t_419, i64 %t_421, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  call void @__omni_c_return_ptr(i8* null)
  %t64_423 = load i64, i64* %var_pos_410
  %t_424 = add i64 %t64_423, 1
  store i64 %t_424, i64* %var_pos_410
  call void @__omni_c_return_ptr(i8* null)
  %t64_425 = load i64, i64* %var_v_409
  %t64_426 = load i64, i64* %var_d2_416
  %t_427 = mul i64 %t64_426, 100
  %t_428 = sub i64 %t64_425, %t_427
  store i64 %t_428, i64* %var_rem_429
  call void @__omni_c_return_ptr(i8* null)
  %t64_430 = load i64, i64* %var_rem_429
  %div_zero_432 = icmp eq i64 10, 0
  %safe_div_432 = select i1 %div_zero_432, i64 1, i64 10
  %t_432 = sdiv i64 %t64_430, %safe_div_432
  store i64 %t_432, i64* %var_d1_433
  %t64_434 = load i64, i64* %param_dst_404
  %t64_435 = load i64, i64* %var_pos_410
  %t_436 = add i64 %t64_434, %t64_435
  %t64_437 = load i64, i64* %var_d1_433
  %t_438 = add i64 48, %t64_437
  %fn_ret_439 = call i64 @__omni_cyw_priv_w8(i64 %t_436, i64 %t_438, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  call void @__omni_c_return_ptr(i8* null)
  %t64_440 = load i64, i64* %var_pos_410
  %t_441 = add i64 %t64_440, 1
  store i64 %t_441, i64* %var_pos_410
  %t64_442 = load i64, i64* %param_dst_404
  %t64_443 = load i64, i64* %var_pos_410
  %t_444 = add i64 %t64_442, %t64_443
  %t64_445 = load i64, i64* %var_rem_429
  %t64_446 = load i64, i64* %var_d1_433
  %t_447 = mul i64 %t64_446, 10
  %t_448 = sub i64 %t64_445, %t_447
  %t_449 = add i64 48, %t_448
  %fn_ret_450 = call i64 @__omni_cyw_priv_w8(i64 %t_444, i64 %t_449, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_451 = load i64, i64* %var_pos_410
  %t_452 = add i64 %t64_451, 1
  ret i64 %t_452
if_merge_12:
  %t64_453 = load i64, i64* %var_v_409
  %cond_454 = icmp sge i64 %t64_453, 10
  br i1 %cond_454, label %if_then_13, label %if_merge_13

if_then_13:
  call void @__omni_c_return_ptr(i8* null)
  %t64_455 = load i64, i64* %var_v_409
  %div_zero_457 = icmp eq i64 10, 0
  %safe_div_457 = select i1 %div_zero_457, i64 1, i64 10
  %t_457 = sdiv i64 %t64_455, %safe_div_457
  store i64 %t_457, i64* %var_d1_458
  %t64_459 = load i64, i64* %param_dst_404
  %t64_460 = load i64, i64* %var_pos_410
  %t_461 = add i64 %t64_459, %t64_460
  %t64_462 = load i64, i64* %var_d1_458
  %t_463 = add i64 48, %t64_462
  %fn_ret_464 = call i64 @__omni_cyw_priv_w8(i64 %t_461, i64 %t_463, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  call void @__omni_c_return_ptr(i8* null)
  %t64_465 = load i64, i64* %var_pos_410
  %t_466 = add i64 %t64_465, 1
  store i64 %t_466, i64* %var_pos_410
  %t64_467 = load i64, i64* %param_dst_404
  %t64_468 = load i64, i64* %var_pos_410
  %t_469 = add i64 %t64_467, %t64_468
  %t64_470 = load i64, i64* %var_v_409
  %t64_471 = load i64, i64* %var_d1_458
  %t_472 = mul i64 %t64_471, 10
  %t_473 = sub i64 %t64_470, %t_472
  %t_474 = add i64 48, %t_473
  %fn_ret_475 = call i64 @__omni_cyw_priv_w8(i64 %t_469, i64 %t_474, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_476 = load i64, i64* %var_pos_410
  %t_477 = add i64 %t64_476, 1
  ret i64 %t_477
if_merge_13:
  %t64_478 = load i64, i64* %param_dst_404
  %t64_479 = load i64, i64* %var_v_409
  %t_480 = add i64 48, %t64_479
  %fn_ret_481 = call i64 @__omni_cyw_priv_w8(i64 %t64_478, i64 %t_480, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  ret i64 1
}


define void @__omni_trampoline_write_u8_dec(i8* %raw_arg) {
entry:
  %raw_int = ptrtoint i8* %raw_arg to i64
  %res = call i64 @__omni_cyw_priv_write_u8_dec(i64 %raw_int, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  ret void
}

define void @__omni_trampoline_pack_write_u8_dec(i8* %raw_pack) {
entry:
  %p64 = bitcast i8* %raw_pack to i64*
  %p0 = getelementptr inbounds i64, i64* %p64, i64 0
  %a0 = load i64, i64* %p0
  %p1 = getelementptr inbounds i64, i64* %p64, i64 1
  %a1 = load i64, i64* %p1
  %p2 = getelementptr inbounds i64, i64* %p64, i64 2
  %a2 = load i64, i64* %p2
  %p3 = getelementptr inbounds i64, i64* %p64, i64 3
  %a3 = load i64, i64* %p3
  %p4 = getelementptr inbounds i64, i64* %p64, i64 4
  %a4 = load i64, i64* %p4
  %p5 = getelementptr inbounds i64, i64* %p64, i64 5
  %a5 = load i64, i64* %p5
  call void @free(i8* %raw_pack)
  %res = call i64 @__omni_cyw_priv_write_u8_dec(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5)
  ret void
}

define i64 @__omni_cyw_priv_write_u16_dec(i64 %arg0, i64 %arg1, i64 %arg2, i64 %arg3, i64 %arg4, i64 %arg5) {
entry:
  %param_dst_483 = alloca i64
  %param_val_485 = alloca i64
  %var_v_488 = alloca i64
  %var_tmp0_493 = alloca i64
  %var_digits_494 = alloca i64
  %var_cur_496 = alloca i64
  %var_q_502 = alloca i64
  %var_r_507 = alloca i64
  %param_unset_dst_482 = icmp eq i64 %arg0, -581005069656919554
  %param_dsel_dst_482 = select i1 %param_unset_dst_482, i64 0, i64 %arg0
  store i64 %param_dsel_dst_482, i64* %param_dst_483
  %param_unset_val_484 = icmp eq i64 %arg1, -581005069656919554
  %param_dsel_val_484 = select i1 %param_unset_val_484, i64 0, i64 %arg1
  store i64 %param_dsel_val_484, i64* %param_val_485
  call void @__omni_c_return_ptr(i8* null)
  %t64_486 = load i64, i64* %param_val_485
  %t_487 = and i64 %t64_486, 65535
  store i64 %t_487, i64* %var_v_488
  %t64_489 = load i64, i64* %var_v_488
  %cond_490 = icmp eq i64 %t64_489, 0
  br i1 %cond_490, label %if_then_14, label %if_merge_14

if_then_14:
  %t64_491 = load i64, i64* %param_dst_483
  %fn_ret_492 = call i64 @__omni_cyw_priv_w8(i64 %t64_491, i64 48, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  ret i64 1
if_merge_14:
  call void @__omni_c_return_ptr(i8* null)
  store i64 0, i64* %var_tmp0_493
  call void @__omni_c_return_ptr(i8* null)
  store i64 0, i64* %var_digits_494
  call void @__omni_c_return_ptr(i8* null)
  %t64_495 = load i64, i64* %var_v_488
  store i64 %t64_495, i64* %var_cur_496
  br label %while_cond_15

while_cond_15:
  %t64_497 = load i64, i64* %var_cur_496
  %cond_498 = icmp sgt i64 %t64_497, 0
  br i1 %cond_498, label %while_body_15, label %while_exit_15

while_body_15:
  call void @__omni_c_return_ptr(i8* null)
  %t64_499 = load i64, i64* %var_cur_496
  %div_zero_501 = icmp eq i64 10, 0
  %safe_div_501 = select i1 %div_zero_501, i64 1, i64 10
  %t_501 = sdiv i64 %t64_499, %safe_div_501
  store i64 %t_501, i64* %var_q_502
  call void @__omni_c_return_ptr(i8* null)
  %t64_503 = load i64, i64* %var_cur_496
  %t64_504 = load i64, i64* %var_q_502
  %t_505 = mul i64 %t64_504, 10
  %t_506 = sub i64 %t64_503, %t_505
  store i64 %t_506, i64* %var_r_507
  %t64_508 = load i64, i64* %param_dst_483
  %t_509 = add i64 %t64_508, 8
  %t64_510 = load i64, i64* %var_digits_494
  %t_511 = add i64 %t_509, %t64_510
  %t64_512 = load i64, i64* %var_r_507
  %t_513 = add i64 48, %t64_512
  %fn_ret_514 = call i64 @__omni_cyw_priv_w8(i64 %t_511, i64 %t_513, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  call void @__omni_c_return_ptr(i8* null)
  %t64_515 = load i64, i64* %var_digits_494
  %t_516 = add i64 %t64_515, 1
  store i64 %t_516, i64* %var_digits_494
  call void @__omni_c_return_ptr(i8* null)
  %t64_517 = load i64, i64* %var_q_502
  store i64 %t64_517, i64* %var_cur_496
  br label %while_cond_15

while_exit_15:
  br label %while_cond_16

while_cond_16:
  %t64_518 = load i64, i64* %var_tmp0_493
  %t64_519 = load i64, i64* %var_digits_494
  %cond_520 = icmp slt i64 %t64_518, %t64_519
  br i1 %cond_520, label %while_body_16, label %while_exit_16

while_body_16:
  %t64_521 = load i64, i64* %param_dst_483
  %t64_522 = load i64, i64* %var_tmp0_493
  %t_523 = add i64 %t64_521, %t64_522
  %t64_524 = load i64, i64* %param_dst_483
  %t_525 = add i64 %t64_524, 8
  %t64_526 = load i64, i64* %var_digits_494
  %t_527 = sub i64 %t64_526, 1
  %t64_528 = load i64, i64* %var_tmp0_493
  %t_529 = sub i64 %t_527, %t64_528
  %t_530 = add i64 %t_525, %t_529
  %fn_ret_531 = call i64 @__omni_cyw_priv_r8(i64 %t_530, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %fn_ret_532 = call i64 @__omni_cyw_priv_w8(i64 %t_523, i64 %fn_ret_531, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  call void @__omni_c_return_ptr(i8* null)
  %t64_533 = load i64, i64* %var_tmp0_493
  %t_534 = add i64 %t64_533, 1
  store i64 %t_534, i64* %var_tmp0_493
  br label %while_cond_16

while_exit_16:
  %t64_535 = load i64, i64* %var_digits_494
  ret i64 %t64_535
}


define void @__omni_trampoline_write_u16_dec(i8* %raw_arg) {
entry:
  %raw_int = ptrtoint i8* %raw_arg to i64
  %res = call i64 @__omni_cyw_priv_write_u16_dec(i64 %raw_int, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  ret void
}

define void @__omni_trampoline_pack_write_u16_dec(i8* %raw_pack) {
entry:
  %p64 = bitcast i8* %raw_pack to i64*
  %p0 = getelementptr inbounds i64, i64* %p64, i64 0
  %a0 = load i64, i64* %p0
  %p1 = getelementptr inbounds i64, i64* %p64, i64 1
  %a1 = load i64, i64* %p1
  %p2 = getelementptr inbounds i64, i64* %p64, i64 2
  %a2 = load i64, i64* %p2
  %p3 = getelementptr inbounds i64, i64* %p64, i64 3
  %a3 = load i64, i64* %p3
  %p4 = getelementptr inbounds i64, i64* %p64, i64 4
  %a4 = load i64, i64* %p4
  %p5 = getelementptr inbounds i64, i64* %p64, i64 5
  %a5 = load i64, i64* %p5
  call void @free(i8* %raw_pack)
  %res = call i64 @__omni_cyw_priv_write_u16_dec(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5)
  ret void
}

define i64 @__omni_cyw_priv_format_ip_str(i64 %arg0, i64 %arg1, i64 %arg2, i64 %arg3, i64 %arg4, i64 %arg5) {
entry:
  %param_ip_ptr_537 = alloca i64
  %param_out_ptr_539 = alloca i64
  %var_pos_540 = alloca i64
  %param_unset_ip_ptr_536 = icmp eq i64 %arg0, -581005069656919554
  %param_dsel_ip_ptr_536 = select i1 %param_unset_ip_ptr_536, i64 0, i64 %arg0
  store i64 %param_dsel_ip_ptr_536, i64* %param_ip_ptr_537
  %param_unset_out_ptr_538 = icmp eq i64 %arg1, -581005069656919554
  %param_dsel_out_ptr_538 = select i1 %param_unset_out_ptr_538, i64 0, i64 %arg1
  store i64 %param_dsel_out_ptr_538, i64* %param_out_ptr_539
  call void @__omni_c_return_ptr(i8* null)
  store i64 0, i64* %var_pos_540
  call void @__omni_c_return_ptr(i8* null)
  %t64_541 = load i64, i64* %var_pos_540
  %t64_542 = load i64, i64* %param_out_ptr_539
  %t64_543 = load i64, i64* %var_pos_540
  %t_544 = add i64 %t64_542, %t64_543
  %t64_545 = load i64, i64* %param_ip_ptr_537
  %fn_ret_546 = call i64 @__omni_cyw_priv_r8(i64 %t64_545, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %fn_ret_547 = call i64 @__omni_cyw_priv_write_u8_dec(i64 %t_544, i64 %fn_ret_546, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t_548 = add i64 %t64_541, %fn_ret_547
  store i64 %t_548, i64* %var_pos_540
  %t64_549 = load i64, i64* %param_out_ptr_539
  %t64_550 = load i64, i64* %var_pos_540
  %t_551 = add i64 %t64_549, %t64_550
  %fn_ret_552 = call i64 @__omni_cyw_priv_w8(i64 %t_551, i64 46, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  call void @__omni_c_return_ptr(i8* null)
  %t64_553 = load i64, i64* %var_pos_540
  %t_554 = add i64 %t64_553, 1
  store i64 %t_554, i64* %var_pos_540
  call void @__omni_c_return_ptr(i8* null)
  %t64_555 = load i64, i64* %var_pos_540
  %t64_556 = load i64, i64* %param_out_ptr_539
  %t64_557 = load i64, i64* %var_pos_540
  %t_558 = add i64 %t64_556, %t64_557
  %t64_559 = load i64, i64* %param_ip_ptr_537
  %t_560 = add i64 %t64_559, 1
  %fn_ret_561 = call i64 @__omni_cyw_priv_r8(i64 %t_560, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %fn_ret_562 = call i64 @__omni_cyw_priv_write_u8_dec(i64 %t_558, i64 %fn_ret_561, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t_563 = add i64 %t64_555, %fn_ret_562
  store i64 %t_563, i64* %var_pos_540
  %t64_564 = load i64, i64* %param_out_ptr_539
  %t64_565 = load i64, i64* %var_pos_540
  %t_566 = add i64 %t64_564, %t64_565
  %fn_ret_567 = call i64 @__omni_cyw_priv_w8(i64 %t_566, i64 46, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  call void @__omni_c_return_ptr(i8* null)
  %t64_568 = load i64, i64* %var_pos_540
  %t_569 = add i64 %t64_568, 1
  store i64 %t_569, i64* %var_pos_540
  call void @__omni_c_return_ptr(i8* null)
  %t64_570 = load i64, i64* %var_pos_540
  %t64_571 = load i64, i64* %param_out_ptr_539
  %t64_572 = load i64, i64* %var_pos_540
  %t_573 = add i64 %t64_571, %t64_572
  %t64_574 = load i64, i64* %param_ip_ptr_537
  %t_575 = add i64 %t64_574, 2
  %fn_ret_576 = call i64 @__omni_cyw_priv_r8(i64 %t_575, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %fn_ret_577 = call i64 @__omni_cyw_priv_write_u8_dec(i64 %t_573, i64 %fn_ret_576, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t_578 = add i64 %t64_570, %fn_ret_577
  store i64 %t_578, i64* %var_pos_540
  %t64_579 = load i64, i64* %param_out_ptr_539
  %t64_580 = load i64, i64* %var_pos_540
  %t_581 = add i64 %t64_579, %t64_580
  %fn_ret_582 = call i64 @__omni_cyw_priv_w8(i64 %t_581, i64 46, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  call void @__omni_c_return_ptr(i8* null)
  %t64_583 = load i64, i64* %var_pos_540
  %t_584 = add i64 %t64_583, 1
  store i64 %t_584, i64* %var_pos_540
  call void @__omni_c_return_ptr(i8* null)
  %t64_585 = load i64, i64* %var_pos_540
  %t64_586 = load i64, i64* %param_out_ptr_539
  %t64_587 = load i64, i64* %var_pos_540
  %t_588 = add i64 %t64_586, %t64_587
  %t64_589 = load i64, i64* %param_ip_ptr_537
  %t_590 = add i64 %t64_589, 3
  %fn_ret_591 = call i64 @__omni_cyw_priv_r8(i64 %t_590, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %fn_ret_592 = call i64 @__omni_cyw_priv_write_u8_dec(i64 %t_588, i64 %fn_ret_591, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t_593 = add i64 %t64_585, %fn_ret_592
  store i64 %t_593, i64* %var_pos_540
  %t64_594 = load i64, i64* %param_out_ptr_539
  %t64_595 = load i64, i64* %var_pos_540
  %t_596 = add i64 %t64_594, %t64_595
  %fn_ret_597 = call i64 @__omni_cyw_priv_w8(i64 %t_596, i64 0, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_598 = load i64, i64* %var_pos_540
  ret i64 %t64_598
}


define void @__omni_trampoline_format_ip_str(i8* %raw_arg) {
entry:
  %raw_int = ptrtoint i8* %raw_arg to i64
  %res = call i64 @__omni_cyw_priv_format_ip_str(i64 %raw_int, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  ret void
}

define void @__omni_trampoline_pack_format_ip_str(i8* %raw_pack) {
entry:
  %p64 = bitcast i8* %raw_pack to i64*
  %p0 = getelementptr inbounds i64, i64* %p64, i64 0
  %a0 = load i64, i64* %p0
  %p1 = getelementptr inbounds i64, i64* %p64, i64 1
  %a1 = load i64, i64* %p1
  %p2 = getelementptr inbounds i64, i64* %p64, i64 2
  %a2 = load i64, i64* %p2
  %p3 = getelementptr inbounds i64, i64* %p64, i64 3
  %a3 = load i64, i64* %p3
  %p4 = getelementptr inbounds i64, i64* %p64, i64 4
  %a4 = load i64, i64* %p4
  %p5 = getelementptr inbounds i64, i64* %p64, i64 5
  %a5 = load i64, i64* %p5
  call void @free(i8* %raw_pack)
  %res = call i64 @__omni_cyw_priv_format_ip_str(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5)
  ret void
}

define i64 @__omni_cyw_priv_inet_csum_acc(i64 %arg0, i64 %arg1, i64 %arg2, i64 %arg3, i64 %arg4, i64 %arg5) {
entry:
  %param_sum_in_600 = alloca i64
  %param_ptr_602 = alloca i64
  %param_len_604 = alloca i64
  %var_sum_606 = alloca i64
  %var_i_607 = alloca i64
  %param_unset_sum_in_599 = icmp eq i64 %arg0, -581005069656919554
  %param_dsel_sum_in_599 = select i1 %param_unset_sum_in_599, i64 0, i64 %arg0
  store i64 %param_dsel_sum_in_599, i64* %param_sum_in_600
  %param_unset_ptr_601 = icmp eq i64 %arg1, -581005069656919554
  %param_dsel_ptr_601 = select i1 %param_unset_ptr_601, i64 0, i64 %arg1
  store i64 %param_dsel_ptr_601, i64* %param_ptr_602
  %param_unset_len_603 = icmp eq i64 %arg2, -581005069656919554
  %param_dsel_len_603 = select i1 %param_unset_len_603, i64 0, i64 %arg2
  store i64 %param_dsel_len_603, i64* %param_len_604
  call void @__omni_c_return_ptr(i8* null)
  %t64_605 = load i64, i64* %param_sum_in_600
  store i64 %t64_605, i64* %var_sum_606
  call void @__omni_c_return_ptr(i8* null)
  store i64 0, i64* %var_i_607
  br label %while_cond_17

while_cond_17:
  %t64_608 = load i64, i64* %var_i_607
  %t_609 = add i64 %t64_608, 1
  %t64_610 = load i64, i64* %param_len_604
  %cond_611 = icmp slt i64 %t_609, %t64_610
  br i1 %cond_611, label %while_body_17, label %while_exit_17

while_body_17:
  call void @__omni_c_return_ptr(i8* null)
  %t64_612 = load i64, i64* %var_sum_606
  %t64_613 = load i64, i64* %param_ptr_602
  %t64_614 = load i64, i64* %var_i_607
  %t_615 = add i64 %t64_613, %t64_614
  %fn_ret_616 = call i64 @__omni_cyw_priv_r16_be(i64 %t_615, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t_617 = add i64 %t64_612, %fn_ret_616
  store i64 %t_617, i64* %var_sum_606
  call void @__omni_c_return_ptr(i8* null)
  %t64_618 = load i64, i64* %var_i_607
  %t_619 = add i64 %t64_618, 2
  store i64 %t_619, i64* %var_i_607
  br label %while_cond_17

while_exit_17:
  %t64_620 = load i64, i64* %var_i_607
  %t64_621 = load i64, i64* %param_len_604
  %cond_622 = icmp slt i64 %t64_620, %t64_621
  br i1 %cond_622, label %if_then_18, label %if_merge_18

if_then_18:
  call void @__omni_c_return_ptr(i8* null)
  %t64_623 = load i64, i64* %var_sum_606
  %t64_624 = load i64, i64* %param_ptr_602
  %t64_625 = load i64, i64* %var_i_607
  %t_626 = add i64 %t64_624, %t64_625
  %fn_ret_627 = call i64 @__omni_cyw_priv_r8(i64 %t_626, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t_628 = shl i64 %fn_ret_627, 8
  %t_629 = add i64 %t64_623, %t_628
  store i64 %t_629, i64* %var_sum_606
  br label %if_merge_18

if_merge_18:
  %t64_630 = load i64, i64* %var_sum_606
  ret i64 %t64_630
}


define void @__omni_trampoline_inet_csum_acc(i8* %raw_arg) {
entry:
  %raw_int = ptrtoint i8* %raw_arg to i64
  %res = call i64 @__omni_cyw_priv_inet_csum_acc(i64 %raw_int, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  ret void
}

define void @__omni_trampoline_pack_inet_csum_acc(i8* %raw_pack) {
entry:
  %p64 = bitcast i8* %raw_pack to i64*
  %p0 = getelementptr inbounds i64, i64* %p64, i64 0
  %a0 = load i64, i64* %p0
  %p1 = getelementptr inbounds i64, i64* %p64, i64 1
  %a1 = load i64, i64* %p1
  %p2 = getelementptr inbounds i64, i64* %p64, i64 2
  %a2 = load i64, i64* %p2
  %p3 = getelementptr inbounds i64, i64* %p64, i64 3
  %a3 = load i64, i64* %p3
  %p4 = getelementptr inbounds i64, i64* %p64, i64 4
  %a4 = load i64, i64* %p4
  %p5 = getelementptr inbounds i64, i64* %p64, i64 5
  %a5 = load i64, i64* %p5
  call void @free(i8* %raw_pack)
  %res = call i64 @__omni_cyw_priv_inet_csum_acc(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5)
  ret void
}

define i64 @__omni_cyw_priv_inet_csum_finish(i64 %arg0, i64 %arg1, i64 %arg2, i64 %arg3, i64 %arg4, i64 %arg5) {
entry:
  %param_sum_in_632 = alloca i64
  %var_s_634 = alloca i64
  %param_unset_sum_in_631 = icmp eq i64 %arg0, -581005069656919554
  %param_dsel_sum_in_631 = select i1 %param_unset_sum_in_631, i64 0, i64 %arg0
  store i64 %param_dsel_sum_in_631, i64* %param_sum_in_632
  call void @__omni_c_return_ptr(i8* null)
  %t64_633 = load i64, i64* %param_sum_in_632
  store i64 %t64_633, i64* %var_s_634
  br label %while_cond_19

while_cond_19:
  %t64_635 = load i64, i64* %var_s_634
  %t_636 = ashr i64 %t64_635, 16
  %cond_637 = icmp ne i64 %t_636, 0
  br i1 %cond_637, label %while_body_19, label %while_exit_19

while_body_19:
  call void @__omni_c_return_ptr(i8* null)
  %t64_638 = load i64, i64* %var_s_634
  %t_639 = and i64 %t64_638, 65535
  %t64_640 = load i64, i64* %var_s_634
  %t_641 = ashr i64 %t64_640, 16
  %t_642 = add i64 %t_639, %t_641
  store i64 %t_642, i64* %var_s_634
  br label %while_cond_19

while_exit_19:
  %t64_643 = load i64, i64* %var_s_634
  %t_644 = xor i64 %t64_643, 65535
  %t_645 = and i64 %t_644, 65535
  ret i64 %t_645
}


define void @__omni_trampoline_inet_csum_finish(i8* %raw_arg) {
entry:
  %raw_int = ptrtoint i8* %raw_arg to i64
  %res = call i64 @__omni_cyw_priv_inet_csum_finish(i64 %raw_int, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  ret void
}

define void @__omni_trampoline_pack_inet_csum_finish(i8* %raw_pack) {
entry:
  %p64 = bitcast i8* %raw_pack to i64*
  %p0 = getelementptr inbounds i64, i64* %p64, i64 0
  %a0 = load i64, i64* %p0
  %p1 = getelementptr inbounds i64, i64* %p64, i64 1
  %a1 = load i64, i64* %p1
  %p2 = getelementptr inbounds i64, i64* %p64, i64 2
  %a2 = load i64, i64* %p2
  %p3 = getelementptr inbounds i64, i64* %p64, i64 3
  %a3 = load i64, i64* %p3
  %p4 = getelementptr inbounds i64, i64* %p64, i64 4
  %a4 = load i64, i64* %p4
  %p5 = getelementptr inbounds i64, i64* %p64, i64 5
  %a5 = load i64, i64* %p5
  call void @free(i8* %raw_pack)
  %res = call i64 @__omni_cyw_priv_inet_csum_finish(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5)
  ret void
}

define i64 @__omni_cyw_priv_l4_checksum(i64 %arg0, i64 %arg1, i64 %arg2, i64 %arg3, i64 %arg4, i64 %arg5) {
entry:
  %param_sip_647 = alloca i64
  %param_dip_649 = alloca i64
  %param_proto_651 = alloca i64
  %param_l4_ptr_653 = alloca i64
  %param_l4_len_655 = alloca i64
  %var_sum_675 = alloca i64
  %param_unset_sip_646 = icmp eq i64 %arg0, -581005069656919554
  %param_dsel_sip_646 = select i1 %param_unset_sip_646, i64 0, i64 %arg0
  store i64 %param_dsel_sip_646, i64* %param_sip_647
  %param_unset_dip_648 = icmp eq i64 %arg1, -581005069656919554
  %param_dsel_dip_648 = select i1 %param_unset_dip_648, i64 0, i64 %arg1
  store i64 %param_dsel_dip_648, i64* %param_dip_649
  %param_unset_proto_650 = icmp eq i64 %arg2, -581005069656919554
  %param_dsel_proto_650 = select i1 %param_unset_proto_650, i64 0, i64 %arg2
  store i64 %param_dsel_proto_650, i64* %param_proto_651
  %param_unset_l4_ptr_652 = icmp eq i64 %arg3, -581005069656919554
  %param_dsel_l4_ptr_652 = select i1 %param_unset_l4_ptr_652, i64 0, i64 %arg3
  store i64 %param_dsel_l4_ptr_652, i64* %param_l4_ptr_653
  %param_unset_l4_len_654 = icmp eq i64 %arg4, -581005069656919554
  %param_dsel_l4_len_654 = select i1 %param_unset_l4_len_654, i64 0, i64 %arg4
  store i64 %param_dsel_l4_len_654, i64* %param_l4_len_655
  call void @__omni_c_return_ptr(i8* null)
  %t64_656 = load i64, i64* %param_sip_647
  %fn_ret_657 = call i64 @__omni_cyw_priv_r16_be(i64 %t64_656, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_658 = load i64, i64* %param_sip_647
  %t_659 = add i64 %t64_658, 2
  %fn_ret_660 = call i64 @__omni_cyw_priv_r16_be(i64 %t_659, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t_661 = add i64 %fn_ret_657, %fn_ret_660
  %t64_662 = load i64, i64* %param_dip_649
  %fn_ret_663 = call i64 @__omni_cyw_priv_r16_be(i64 %t64_662, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t_664 = add i64 %t_661, %fn_ret_663
  %t64_665 = load i64, i64* %param_dip_649
  %t_666 = add i64 %t64_665, 2
  %fn_ret_667 = call i64 @__omni_cyw_priv_r16_be(i64 %t_666, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t_668 = add i64 %t_664, %fn_ret_667
  %t64_669 = load i64, i64* %param_proto_651
  %t_670 = and i64 %t64_669, 255
  %t_671 = add i64 %t_668, %t_670
  %t64_672 = load i64, i64* %param_l4_len_655
  %t_673 = and i64 %t64_672, 65535
  %t_674 = add i64 %t_671, %t_673
  store i64 %t_674, i64* %var_sum_675
  call void @__omni_c_return_ptr(i8* null)
  %t64_676 = load i64, i64* %var_sum_675
  %t64_677 = load i64, i64* %param_l4_ptr_653
  %t64_678 = load i64, i64* %param_l4_len_655
  %fn_ret_679 = call i64 @__omni_cyw_priv_inet_csum_acc(i64 %t64_676, i64 %t64_677, i64 %t64_678, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  store i64 %fn_ret_679, i64* %var_sum_675
  %t64_680 = load i64, i64* %var_sum_675
  %fn_ret_681 = call i64 @__omni_cyw_priv_inet_csum_finish(i64 %t64_680, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  ret i64 %fn_ret_681
}


define void @__omni_trampoline_l4_checksum(i8* %raw_arg) {
entry:
  %raw_int = ptrtoint i8* %raw_arg to i64
  %res = call i64 @__omni_cyw_priv_l4_checksum(i64 %raw_int, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  ret void
}

define void @__omni_trampoline_pack_l4_checksum(i8* %raw_pack) {
entry:
  %p64 = bitcast i8* %raw_pack to i64*
  %p0 = getelementptr inbounds i64, i64* %p64, i64 0
  %a0 = load i64, i64* %p0
  %p1 = getelementptr inbounds i64, i64* %p64, i64 1
  %a1 = load i64, i64* %p1
  %p2 = getelementptr inbounds i64, i64* %p64, i64 2
  %a2 = load i64, i64* %p2
  %p3 = getelementptr inbounds i64, i64* %p64, i64 3
  %a3 = load i64, i64* %p3
  %p4 = getelementptr inbounds i64, i64* %p64, i64 4
  %a4 = load i64, i64* %p4
  %p5 = getelementptr inbounds i64, i64* %p64, i64 5
  %a5 = load i64, i64* %p5
  call void @free(i8* %raw_pack)
  %res = call i64 @__omni_cyw_priv_l4_checksum(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5)
  ret void
}

define i64 @__omni_cyw_priv_rotl32(i64 %arg0, i64 %arg1, i64 %arg2, i64 %arg3, i64 %arg4, i64 %arg5) {
entry:
  %param_x_683 = alloca i64
  %param_n_685 = alloca i64
  %var_u_688 = alloca i64
  %var_hi_693 = alloca i64
  %var_lo_698 = alloca i64
  %param_unset_x_682 = icmp eq i64 %arg0, -581005069656919554
  %param_dsel_x_682 = select i1 %param_unset_x_682, i64 0, i64 %arg0
  store i64 %param_dsel_x_682, i64* %param_x_683
  %param_unset_n_684 = icmp eq i64 %arg1, -581005069656919554
  %param_dsel_n_684 = select i1 %param_unset_n_684, i64 0, i64 %arg1
  store i64 %param_dsel_n_684, i64* %param_n_685
  call void @__omni_c_return_ptr(i8* null)
  %t64_686 = load i64, i64* %param_x_683
  %t_687 = and i64 %t64_686, 4294967295
  store i64 %t_687, i64* %var_u_688
  call void @__omni_c_return_ptr(i8* null)
  %t64_689 = load i64, i64* %var_u_688
  %t64_690 = load i64, i64* %param_n_685
  %t_691 = shl i64 %t64_689, %t64_690
  %t_692 = and i64 %t_691, 4294967295
  store i64 %t_692, i64* %var_hi_693
  call void @__omni_c_return_ptr(i8* null)
  %t64_694 = load i64, i64* %var_u_688
  %t64_695 = load i64, i64* %param_n_685
  %t_696 = sub i64 32, %t64_695
  %t_697 = ashr i64 %t64_694, %t_696
  store i64 %t_697, i64* %var_lo_698
  %t64_699 = load i64, i64* %var_hi_693
  %t64_700 = load i64, i64* %var_lo_698
  %t_701 = or i64 %t64_699, %t64_700
  %t_702 = and i64 %t_701, 4294967295
  ret i64 %t_702
}


define void @__omni_trampoline_rotl32(i8* %raw_arg) {
entry:
  %raw_int = ptrtoint i8* %raw_arg to i64
  %res = call i64 @__omni_cyw_priv_rotl32(i64 %raw_int, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  ret void
}

define void @__omni_trampoline_pack_rotl32(i8* %raw_pack) {
entry:
  %p64 = bitcast i8* %raw_pack to i64*
  %p0 = getelementptr inbounds i64, i64* %p64, i64 0
  %a0 = load i64, i64* %p0
  %p1 = getelementptr inbounds i64, i64* %p64, i64 1
  %a1 = load i64, i64* %p1
  %p2 = getelementptr inbounds i64, i64* %p64, i64 2
  %a2 = load i64, i64* %p2
  %p3 = getelementptr inbounds i64, i64* %p64, i64 3
  %a3 = load i64, i64* %p3
  %p4 = getelementptr inbounds i64, i64* %p64, i64 4
  %a4 = load i64, i64* %p4
  %p5 = getelementptr inbounds i64, i64* %p64, i64 5
  %a5 = load i64, i64* %p5
  call void @free(i8* %raw_pack)
  %res = call i64 @__omni_cyw_priv_rotl32(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5)
  ret void
}

define i64 @__omni_cyw_priv_b64_char(i64 %arg0, i64 %arg1, i64 %arg2, i64 %arg3, i64 %arg4, i64 %arg5) {
entry:
  %param_idx_704 = alloca i64
  %var_v_707 = alloca i64
  %param_unset_idx_703 = icmp eq i64 %arg0, -581005069656919554
  %param_dsel_idx_703 = select i1 %param_unset_idx_703, i64 0, i64 %arg0
  store i64 %param_dsel_idx_703, i64* %param_idx_704
  call void @__omni_c_return_ptr(i8* null)
  %t64_705 = load i64, i64* %param_idx_704
  %t_706 = and i64 %t64_705, 63
  store i64 %t_706, i64* %var_v_707
  %t64_708 = load i64, i64* %var_v_707
  %cond_709 = icmp slt i64 %t64_708, 26
  br i1 %cond_709, label %if_then_20, label %if_merge_20

if_then_20:
  %t64_710 = load i64, i64* %var_v_707
  %t_711 = add i64 65, %t64_710
  ret i64 %t_711
if_merge_20:
  %t64_712 = load i64, i64* %var_v_707
  %cond_713 = icmp slt i64 %t64_712, 52
  br i1 %cond_713, label %if_then_21, label %if_merge_21

if_then_21:
  %t64_714 = load i64, i64* %var_v_707
  %t_715 = sub i64 %t64_714, 26
  %t_716 = add i64 97, %t_715
  ret i64 %t_716
if_merge_21:
  %t64_717 = load i64, i64* %var_v_707
  %cond_718 = icmp slt i64 %t64_717, 62
  br i1 %cond_718, label %if_then_22, label %if_merge_22

if_then_22:
  %t64_719 = load i64, i64* %var_v_707
  %t_720 = sub i64 %t64_719, 52
  %t_721 = add i64 48, %t_720
  ret i64 %t_721
if_merge_22:
  %t64_722 = load i64, i64* %var_v_707
  %cond_723 = icmp eq i64 %t64_722, 62
  br i1 %cond_723, label %if_then_23, label %if_merge_23

if_then_23:
  ret i64 43
if_merge_23:
  ret i64 47
}


define void @__omni_trampoline_b64_char(i8* %raw_arg) {
entry:
  %raw_int = ptrtoint i8* %raw_arg to i64
  %res = call i64 @__omni_cyw_priv_b64_char(i64 %raw_int, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  ret void
}

define void @__omni_trampoline_pack_b64_char(i8* %raw_pack) {
entry:
  %p64 = bitcast i8* %raw_pack to i64*
  %p0 = getelementptr inbounds i64, i64* %p64, i64 0
  %a0 = load i64, i64* %p0
  %p1 = getelementptr inbounds i64, i64* %p64, i64 1
  %a1 = load i64, i64* %p1
  %p2 = getelementptr inbounds i64, i64* %p64, i64 2
  %a2 = load i64, i64* %p2
  %p3 = getelementptr inbounds i64, i64* %p64, i64 3
  %a3 = load i64, i64* %p3
  %p4 = getelementptr inbounds i64, i64* %p64, i64 4
  %a4 = load i64, i64* %p4
  %p5 = getelementptr inbounds i64, i64* %p64, i64 5
  %a5 = load i64, i64* %p5
  call void @free(i8* %raw_pack)
  %res = call i64 @__omni_cyw_priv_b64_char(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5)
  ret void
}

define i64 @__omni_fn_cyw_ws_compute_accept(i64 %arg0, i64 %arg1, i64 %arg2, i64 %arg3, i64 %arg4, i64 %arg5) {
entry:
  %param_ctx_725 = alloca i64
  %param_key_ptr_727 = alloca i64
  %param_key_len_729 = alloca i64
  %param_out_b64_731 = alloca i64
  %var_sha_734 = alloca i64
  %var_msg_736 = alloca i64
  %var_wbuf_739 = alloca i64
  %var_dig_742 = alloca i64
  %var_kl_746 = alloca i64
  %var_glen_759 = alloca i64
  %var_total_763 = alloca i64
  %var_nblocks_768 = alloca i64
  %var_bit_len_774 = alloca i64
  %var_h0_788 = alloca i64
  %var_h1_789 = alloca i64
  %var_h2_790 = alloca i64
  %var_h3_791 = alloca i64
  %var_h4_792 = alloca i64
  %var_blk_793 = alloca i64
  %var_bp_801 = alloca i64
  %var_t_802 = alloca i64
  %var_xw_846 = alloca i64
  %var_a_857 = alloca i64
  %var_b_859 = alloca i64
  %var_cc_861 = alloca i64
  %var_d_863 = alloca i64
  %var_e_865 = alloca i64
  %var_f_868 = alloca i64
  %var_k_869 = alloca i64
  %var_wt_914 = alloca i64
  %var_tmp_926 = alloca i64
  %var_gi_977 = alloca i64
  %var_op_978 = alloca i64
  %var_v24_999 = alloca i64
  %var_tail16_1043 = alloca i64
  %param_unset_ctx_724 = icmp eq i64 %arg0, -581005069656919554
  %param_dsel_ctx_724 = select i1 %param_unset_ctx_724, i64 0, i64 %arg0
  store i64 %param_dsel_ctx_724, i64* %param_ctx_725
  %param_unset_key_ptr_726 = icmp eq i64 %arg1, -581005069656919554
  %param_dsel_key_ptr_726 = select i1 %param_unset_key_ptr_726, i64 0, i64 %arg1
  store i64 %param_dsel_key_ptr_726, i64* %param_key_ptr_727
  %param_unset_key_len_728 = icmp eq i64 %arg2, -581005069656919554
  %param_dsel_key_len_728 = select i1 %param_unset_key_len_728, i64 0, i64 %arg2
  store i64 %param_dsel_key_len_728, i64* %param_key_len_729
  %param_unset_out_b64_730 = icmp eq i64 %arg3, -581005069656919554
  %param_dsel_out_b64_730 = select i1 %param_unset_out_b64_730, i64 0, i64 %arg3
  store i64 %param_dsel_out_b64_730, i64* %param_out_b64_731
  call void @__omni_c_return_ptr(i8* null)
  %t64_732 = load i64, i64* %param_ctx_725
  %t_733 = add i64 %t64_732, 2272
  store i64 %t_733, i64* %var_sha_734
  call void @__omni_c_return_ptr(i8* null)
  %t64_735 = load i64, i64* %var_sha_734
  store i64 %t64_735, i64* %var_msg_736
  call void @__omni_c_return_ptr(i8* null)
  %t64_737 = load i64, i64* %var_sha_734
  %t_738 = add i64 %t64_737, 128
  store i64 %t_738, i64* %var_wbuf_739
  call void @__omni_c_return_ptr(i8* null)
  %t64_740 = load i64, i64* %var_sha_734
  %t_741 = add i64 %t64_740, 448
  store i64 %t_741, i64* %var_dig_742
  %t64_743 = load i64, i64* %var_msg_736
  %fn_ret_744 = call i64 @__omni_cyw_priv_bzero(i64 %t64_743, i64 128, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  call void @__omni_c_return_ptr(i8* null)
  %t64_745 = load i64, i64* %param_key_len_729
  store i64 %t64_745, i64* %var_kl_746
  %t64_747 = load i64, i64* %var_kl_746
  %cond_748 = icmp sgt i64 %t64_747, 60
  br i1 %cond_748, label %if_then_24, label %if_merge_24

if_then_24:
  call void @__omni_c_return_ptr(i8* null)
  store i64 60, i64* %var_kl_746
  br label %if_merge_24

if_merge_24:
  %t64_749 = load i64, i64* %var_msg_736
  %t64_750 = load i64, i64* %param_key_ptr_727
  %t64_751 = load i64, i64* %var_kl_746
  %fn_ret_752 = call i64 @__omni_cyw_priv_bcopy(i64 %t64_749, i64 %t64_750, i64 %t64_751, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  call void @__omni_c_return_ptr(i8* null)
  %t64_753 = load i64, i64* %var_msg_736
  %t64_754 = load i64, i64* %var_kl_746
  %t_755 = add i64 %t64_753, %t64_754
  %str_lit_756 = getelementptr inbounds [37 x i8], [37 x i8]* @.str.cyw_4, i64 0, i64 0
  call void @__omni_c_return_ptr(i8* %str_lit_756)
  %p2i64_757 = ptrtoint i8* %str_lit_756 to i64
  %fn_ret_758 = call i64 @__omni_cyw_priv_put_ascii(i64 %t_755, i64 %p2i64_757, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  store i64 %fn_ret_758, i64* %var_glen_759
  call void @__omni_c_return_ptr(i8* null)
  %t64_760 = load i64, i64* %var_kl_746
  %t64_761 = load i64, i64* %var_glen_759
  %t_762 = add i64 %t64_760, %t64_761
  store i64 %t_762, i64* %var_total_763
  %t64_764 = load i64, i64* %var_msg_736
  %t64_765 = load i64, i64* %var_total_763
  %t_766 = add i64 %t64_764, %t64_765
  %fn_ret_767 = call i64 @__omni_cyw_priv_w8(i64 %t_766, i64 128, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  call void @__omni_c_return_ptr(i8* null)
  store i64 1, i64* %var_nblocks_768
  %t64_769 = load i64, i64* %var_total_763
  %t_770 = add i64 %t64_769, 9
  %cond_771 = icmp sgt i64 %t_770, 64
  br i1 %cond_771, label %if_then_25, label %if_merge_25

if_then_25:
  call void @__omni_c_return_ptr(i8* null)
  store i64 2, i64* %var_nblocks_768
  br label %if_merge_25

if_merge_25:
  call void @__omni_c_return_ptr(i8* null)
  %t64_772 = load i64, i64* %var_total_763
  %t_773 = mul i64 %t64_772, 8
  store i64 %t_773, i64* %var_bit_len_774
  %t64_775 = load i64, i64* %var_msg_736
  %t64_776 = load i64, i64* %var_nblocks_768
  %t_777 = mul i64 %t64_776, 64
  %t_778 = add i64 %t64_775, %t_777
  %t_779 = sub i64 %t_778, 8
  %fn_ret_780 = call i64 @__omni_cyw_priv_w32_be(i64 %t_779, i64 0, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_781 = load i64, i64* %var_msg_736
  %t64_782 = load i64, i64* %var_nblocks_768
  %t_783 = mul i64 %t64_782, 64
  %t_784 = add i64 %t64_781, %t_783
  %t_785 = sub i64 %t_784, 4
  %t64_786 = load i64, i64* %var_bit_len_774
  %fn_ret_787 = call i64 @__omni_cyw_priv_w32_be(i64 %t_785, i64 %t64_786, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  call void @__omni_c_return_ptr(i8* null)
  store i64 1732584193, i64* %var_h0_788
  call void @__omni_c_return_ptr(i8* null)
  store i64 4023233417, i64* %var_h1_789
  call void @__omni_c_return_ptr(i8* null)
  store i64 2562383102, i64* %var_h2_790
  call void @__omni_c_return_ptr(i8* null)
  store i64 271733878, i64* %var_h3_791
  call void @__omni_c_return_ptr(i8* null)
  store i64 3285377520, i64* %var_h4_792
  call void @__omni_c_return_ptr(i8* null)
  store i64 0, i64* %var_blk_793
  br label %while_cond_26

while_cond_26:
  %t64_794 = load i64, i64* %var_blk_793
  %t64_795 = load i64, i64* %var_nblocks_768
  %cond_796 = icmp slt i64 %t64_794, %t64_795
  br i1 %cond_796, label %while_body_26, label %while_exit_26

while_body_26:
  call void @__omni_c_return_ptr(i8* null)
  %t64_797 = load i64, i64* %var_msg_736
  %t64_798 = load i64, i64* %var_blk_793
  %t_799 = mul i64 %t64_798, 64
  %t_800 = add i64 %t64_797, %t_799
  store i64 %t_800, i64* %var_bp_801
  call void @__omni_c_return_ptr(i8* null)
  store i64 0, i64* %var_t_802
  br label %while_cond_27

while_cond_27:
  %t64_803 = load i64, i64* %var_t_802
  %cond_804 = icmp slt i64 %t64_803, 16
  br i1 %cond_804, label %while_body_27, label %while_exit_27

while_body_27:
  %t64_805 = load i64, i64* %var_wbuf_739
  %t64_806 = load i64, i64* %var_t_802
  %t_807 = mul i64 %t64_806, 4
  %t_808 = add i64 %t64_805, %t_807
  %t64_809 = load i64, i64* %var_bp_801
  %t64_810 = load i64, i64* %var_t_802
  %t_811 = mul i64 %t64_810, 4
  %t_812 = add i64 %t64_809, %t_811
  %fn_ret_813 = call i64 @__omni_cyw_priv_r32_be(i64 %t_812, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %fn_ret_814 = call i64 @__omni_cyw_priv_w32_le(i64 %t_808, i64 %fn_ret_813, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  call void @__omni_c_return_ptr(i8* null)
  %t64_815 = load i64, i64* %var_t_802
  %t_816 = add i64 %t64_815, 1
  store i64 %t_816, i64* %var_t_802
  br label %while_cond_27

while_exit_27:
  br label %while_cond_28

while_cond_28:
  %t64_817 = load i64, i64* %var_t_802
  %cond_818 = icmp slt i64 %t64_817, 80
  br i1 %cond_818, label %while_body_28, label %while_exit_28

while_body_28:
  call void @__omni_c_return_ptr(i8* null)
  %t64_819 = load i64, i64* %var_wbuf_739
  %t64_820 = load i64, i64* %var_t_802
  %t_821 = sub i64 %t64_820, 3
  %t_822 = mul i64 %t_821, 4
  %t_823 = add i64 %t64_819, %t_822
  %fn_ret_824 = call i64 @__omni_cyw_priv_r32_le(i64 %t_823, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_825 = load i64, i64* %var_wbuf_739
  %t64_826 = load i64, i64* %var_t_802
  %t_827 = sub i64 %t64_826, 8
  %t_828 = mul i64 %t_827, 4
  %t_829 = add i64 %t64_825, %t_828
  %fn_ret_830 = call i64 @__omni_cyw_priv_r32_le(i64 %t_829, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t_831 = xor i64 %fn_ret_824, %fn_ret_830
  %t64_832 = load i64, i64* %var_wbuf_739
  %t64_833 = load i64, i64* %var_t_802
  %t_834 = sub i64 %t64_833, 14
  %t_835 = mul i64 %t_834, 4
  %t_836 = add i64 %t64_832, %t_835
  %fn_ret_837 = call i64 @__omni_cyw_priv_r32_le(i64 %t_836, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t_838 = xor i64 %t_831, %fn_ret_837
  %t64_839 = load i64, i64* %var_wbuf_739
  %t64_840 = load i64, i64* %var_t_802
  %t_841 = sub i64 %t64_840, 16
  %t_842 = mul i64 %t_841, 4
  %t_843 = add i64 %t64_839, %t_842
  %fn_ret_844 = call i64 @__omni_cyw_priv_r32_le(i64 %t_843, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t_845 = xor i64 %t_838, %fn_ret_844
  store i64 %t_845, i64* %var_xw_846
  %t64_847 = load i64, i64* %var_wbuf_739
  %t64_848 = load i64, i64* %var_t_802
  %t_849 = mul i64 %t64_848, 4
  %t_850 = add i64 %t64_847, %t_849
  %t64_851 = load i64, i64* %var_xw_846
  %fn_ret_852 = call i64 @__omni_cyw_priv_rotl32(i64 %t64_851, i64 1, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %fn_ret_853 = call i64 @__omni_cyw_priv_w32_le(i64 %t_850, i64 %fn_ret_852, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  call void @__omni_c_return_ptr(i8* null)
  %t64_854 = load i64, i64* %var_t_802
  %t_855 = add i64 %t64_854, 1
  store i64 %t_855, i64* %var_t_802
  br label %while_cond_28

while_exit_28:
  call void @__omni_c_return_ptr(i8* null)
  %t64_856 = load i64, i64* %var_h0_788
  store i64 %t64_856, i64* %var_a_857
  call void @__omni_c_return_ptr(i8* null)
  %t64_858 = load i64, i64* %var_h1_789
  store i64 %t64_858, i64* %var_b_859
  call void @__omni_c_return_ptr(i8* null)
  %t64_860 = load i64, i64* %var_h2_790
  store i64 %t64_860, i64* %var_cc_861
  call void @__omni_c_return_ptr(i8* null)
  %t64_862 = load i64, i64* %var_h3_791
  store i64 %t64_862, i64* %var_d_863
  call void @__omni_c_return_ptr(i8* null)
  %t64_864 = load i64, i64* %var_h4_792
  store i64 %t64_864, i64* %var_e_865
  call void @__omni_c_return_ptr(i8* null)
  store i64 0, i64* %var_t_802
  br label %while_cond_29

while_cond_29:
  %t64_866 = load i64, i64* %var_t_802
  %cond_867 = icmp slt i64 %t64_866, 80
  br i1 %cond_867, label %while_body_29, label %while_exit_29

while_body_29:
  call void @__omni_c_return_ptr(i8* null)
  store i64 0, i64* %var_f_868
  call void @__omni_c_return_ptr(i8* null)
  store i64 0, i64* %var_k_869
  %t64_870 = load i64, i64* %var_t_802
  %cond_871 = icmp slt i64 %t64_870, 20
  br i1 %cond_871, label %if_then_30, label %if_else_30

if_then_30:
  call void @__omni_c_return_ptr(i8* null)
  %t64_872 = load i64, i64* %var_b_859
  %t64_873 = load i64, i64* %var_cc_861
  %t_874 = and i64 %t64_872, %t64_873
  %t64_875 = load i64, i64* %var_b_859
  %t_876 = xor i64 %t64_875, 4294967295
  %t64_877 = load i64, i64* %var_d_863
  %t_878 = and i64 %t_876, %t64_877
  %t_879 = or i64 %t_874, %t_878
  %t_880 = and i64 %t_879, 4294967295
  store i64 %t_880, i64* %var_f_868
  call void @__omni_c_return_ptr(i8* null)
  store i64 1518500249, i64* %var_k_869
  br label %if_merge_30

if_else_30:
  %t64_881 = load i64, i64* %var_t_802
  %cond_882 = icmp slt i64 %t64_881, 40
  br i1 %cond_882, label %if_then_31, label %if_else_31

if_then_31:
  call void @__omni_c_return_ptr(i8* null)
  %t64_883 = load i64, i64* %var_b_859
  %t64_884 = load i64, i64* %var_cc_861
  %t_885 = xor i64 %t64_883, %t64_884
  %t64_886 = load i64, i64* %var_d_863
  %t_887 = xor i64 %t_885, %t64_886
  %t_888 = and i64 %t_887, 4294967295
  store i64 %t_888, i64* %var_f_868
  call void @__omni_c_return_ptr(i8* null)
  store i64 1859775393, i64* %var_k_869
  br label %if_merge_31

if_else_31:
  %t64_889 = load i64, i64* %var_t_802
  %cond_890 = icmp slt i64 %t64_889, 60
  br i1 %cond_890, label %if_then_32, label %if_else_32

if_then_32:
  call void @__omni_c_return_ptr(i8* null)
  %t64_891 = load i64, i64* %var_b_859
  %t64_892 = load i64, i64* %var_cc_861
  %t_893 = and i64 %t64_891, %t64_892
  %t64_894 = load i64, i64* %var_b_859
  %t64_895 = load i64, i64* %var_d_863
  %t_896 = and i64 %t64_894, %t64_895
  %t_897 = or i64 %t_893, %t_896
  %t64_898 = load i64, i64* %var_cc_861
  %t64_899 = load i64, i64* %var_d_863
  %t_900 = and i64 %t64_898, %t64_899
  %t_901 = or i64 %t_897, %t_900
  %t_902 = and i64 %t_901, 4294967295
  store i64 %t_902, i64* %var_f_868
  call void @__omni_c_return_ptr(i8* null)
  store i64 2400959708, i64* %var_k_869
  br label %if_merge_32

if_else_32:
  call void @__omni_c_return_ptr(i8* null)
  %t64_903 = load i64, i64* %var_b_859
  %t64_904 = load i64, i64* %var_cc_861
  %t_905 = xor i64 %t64_903, %t64_904
  %t64_906 = load i64, i64* %var_d_863
  %t_907 = xor i64 %t_905, %t64_906
  %t_908 = and i64 %t_907, 4294967295
  store i64 %t_908, i64* %var_f_868
  call void @__omni_c_return_ptr(i8* null)
  store i64 3395469782, i64* %var_k_869
  br label %if_merge_32

if_merge_32:
  br label %if_merge_31

if_merge_31:
  br label %if_merge_30

if_merge_30:
  call void @__omni_c_return_ptr(i8* null)
  %t64_909 = load i64, i64* %var_wbuf_739
  %t64_910 = load i64, i64* %var_t_802
  %t_911 = mul i64 %t64_910, 4
  %t_912 = add i64 %t64_909, %t_911
  %fn_ret_913 = call i64 @__omni_cyw_priv_r32_le(i64 %t_912, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  store i64 %fn_ret_913, i64* %var_wt_914
  call void @__omni_c_return_ptr(i8* null)
  %t64_915 = load i64, i64* %var_a_857
  %fn_ret_916 = call i64 @__omni_cyw_priv_rotl32(i64 %t64_915, i64 5, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_917 = load i64, i64* %var_f_868
  %t_918 = add i64 %fn_ret_916, %t64_917
  %t64_919 = load i64, i64* %var_e_865
  %t_920 = add i64 %t_918, %t64_919
  %t64_921 = load i64, i64* %var_k_869
  %t_922 = add i64 %t_920, %t64_921
  %t64_923 = load i64, i64* %var_wt_914
  %t_924 = add i64 %t_922, %t64_923
  %t_925 = and i64 %t_924, 4294967295
  store i64 %t_925, i64* %var_tmp_926
  call void @__omni_c_return_ptr(i8* null)
  %t64_927 = load i64, i64* %var_d_863
  store i64 %t64_927, i64* %var_e_865
  call void @__omni_c_return_ptr(i8* null)
  %t64_928 = load i64, i64* %var_cc_861
  store i64 %t64_928, i64* %var_d_863
  call void @__omni_c_return_ptr(i8* null)
  %t64_929 = load i64, i64* %var_b_859
  %fn_ret_930 = call i64 @__omni_cyw_priv_rotl32(i64 %t64_929, i64 30, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  store i64 %fn_ret_930, i64* %var_cc_861
  call void @__omni_c_return_ptr(i8* null)
  %t64_931 = load i64, i64* %var_a_857
  store i64 %t64_931, i64* %var_b_859
  call void @__omni_c_return_ptr(i8* null)
  %t64_932 = load i64, i64* %var_tmp_926
  store i64 %t64_932, i64* %var_a_857
  call void @__omni_c_return_ptr(i8* null)
  %t64_933 = load i64, i64* %var_t_802
  %t_934 = add i64 %t64_933, 1
  store i64 %t_934, i64* %var_t_802
  br label %while_cond_29

while_exit_29:
  call void @__omni_c_return_ptr(i8* null)
  %t64_935 = load i64, i64* %var_h0_788
  %t64_936 = load i64, i64* %var_a_857
  %t_937 = add i64 %t64_935, %t64_936
  %t_938 = and i64 %t_937, 4294967295
  store i64 %t_938, i64* %var_h0_788
  call void @__omni_c_return_ptr(i8* null)
  %t64_939 = load i64, i64* %var_h1_789
  %t64_940 = load i64, i64* %var_b_859
  %t_941 = add i64 %t64_939, %t64_940
  %t_942 = and i64 %t_941, 4294967295
  store i64 %t_942, i64* %var_h1_789
  call void @__omni_c_return_ptr(i8* null)
  %t64_943 = load i64, i64* %var_h2_790
  %t64_944 = load i64, i64* %var_cc_861
  %t_945 = add i64 %t64_943, %t64_944
  %t_946 = and i64 %t_945, 4294967295
  store i64 %t_946, i64* %var_h2_790
  call void @__omni_c_return_ptr(i8* null)
  %t64_947 = load i64, i64* %var_h3_791
  %t64_948 = load i64, i64* %var_d_863
  %t_949 = add i64 %t64_947, %t64_948
  %t_950 = and i64 %t_949, 4294967295
  store i64 %t_950, i64* %var_h3_791
  call void @__omni_c_return_ptr(i8* null)
  %t64_951 = load i64, i64* %var_h4_792
  %t64_952 = load i64, i64* %var_e_865
  %t_953 = add i64 %t64_951, %t64_952
  %t_954 = and i64 %t_953, 4294967295
  store i64 %t_954, i64* %var_h4_792
  call void @__omni_c_return_ptr(i8* null)
  %t64_955 = load i64, i64* %var_blk_793
  %t_956 = add i64 %t64_955, 1
  store i64 %t_956, i64* %var_blk_793
  br label %while_cond_26

while_exit_26:
  %t64_957 = load i64, i64* %var_dig_742
  %t_958 = add i64 %t64_957, 0
  %t64_959 = load i64, i64* %var_h0_788
  %fn_ret_960 = call i64 @__omni_cyw_priv_w32_be(i64 %t_958, i64 %t64_959, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_961 = load i64, i64* %var_dig_742
  %t_962 = add i64 %t64_961, 4
  %t64_963 = load i64, i64* %var_h1_789
  %fn_ret_964 = call i64 @__omni_cyw_priv_w32_be(i64 %t_962, i64 %t64_963, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_965 = load i64, i64* %var_dig_742
  %t_966 = add i64 %t64_965, 8
  %t64_967 = load i64, i64* %var_h2_790
  %fn_ret_968 = call i64 @__omni_cyw_priv_w32_be(i64 %t_966, i64 %t64_967, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_969 = load i64, i64* %var_dig_742
  %t_970 = add i64 %t64_969, 12
  %t64_971 = load i64, i64* %var_h3_791
  %fn_ret_972 = call i64 @__omni_cyw_priv_w32_be(i64 %t_970, i64 %t64_971, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_973 = load i64, i64* %var_dig_742
  %t_974 = add i64 %t64_973, 16
  %t64_975 = load i64, i64* %var_h4_792
  %fn_ret_976 = call i64 @__omni_cyw_priv_w32_be(i64 %t_974, i64 %t64_975, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  call void @__omni_c_return_ptr(i8* null)
  store i64 0, i64* %var_gi_977
  call void @__omni_c_return_ptr(i8* null)
  store i64 0, i64* %var_op_978
  br label %while_cond_33

while_cond_33:
  %t64_979 = load i64, i64* %var_gi_977
  %cond_980 = icmp slt i64 %t64_979, 18
  br i1 %cond_980, label %while_body_33, label %while_exit_33

while_body_33:
  call void @__omni_c_return_ptr(i8* null)
  %t64_981 = load i64, i64* %var_dig_742
  %t64_982 = load i64, i64* %var_gi_977
  %t_983 = add i64 %t64_981, %t64_982
  %fn_ret_984 = call i64 @__omni_cyw_priv_r8(i64 %t_983, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t_985 = shl i64 %fn_ret_984, 16
  %t64_986 = load i64, i64* %var_dig_742
  %t64_987 = load i64, i64* %var_gi_977
  %t_988 = add i64 %t64_986, %t64_987
  %t_989 = add i64 %t_988, 1
  %fn_ret_990 = call i64 @__omni_cyw_priv_r8(i64 %t_989, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t_991 = shl i64 %fn_ret_990, 8
  %t_992 = or i64 %t_985, %t_991
  %t64_993 = load i64, i64* %var_dig_742
  %t64_994 = load i64, i64* %var_gi_977
  %t_995 = add i64 %t64_993, %t64_994
  %t_996 = add i64 %t_995, 2
  %fn_ret_997 = call i64 @__omni_cyw_priv_r8(i64 %t_996, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t_998 = or i64 %t_992, %fn_ret_997
  store i64 %t_998, i64* %var_v24_999
  %t64_1000 = load i64, i64* %param_out_b64_731
  %t64_1001 = load i64, i64* %var_op_978
  %t_1002 = add i64 %t64_1000, %t64_1001
  %t64_1003 = load i64, i64* %var_v24_999
  %t_1004 = ashr i64 %t64_1003, 18
  %fn_ret_1005 = call i64 @__omni_cyw_priv_b64_char(i64 %t_1004, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %fn_ret_1006 = call i64 @__omni_cyw_priv_w8(i64 %t_1002, i64 %fn_ret_1005, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_1007 = load i64, i64* %param_out_b64_731
  %t64_1008 = load i64, i64* %var_op_978
  %t_1009 = add i64 %t64_1007, %t64_1008
  %t_1010 = add i64 %t_1009, 1
  %t64_1011 = load i64, i64* %var_v24_999
  %t_1012 = ashr i64 %t64_1011, 12
  %fn_ret_1013 = call i64 @__omni_cyw_priv_b64_char(i64 %t_1012, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %fn_ret_1014 = call i64 @__omni_cyw_priv_w8(i64 %t_1010, i64 %fn_ret_1013, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_1015 = load i64, i64* %param_out_b64_731
  %t64_1016 = load i64, i64* %var_op_978
  %t_1017 = add i64 %t64_1015, %t64_1016
  %t_1018 = add i64 %t_1017, 2
  %t64_1019 = load i64, i64* %var_v24_999
  %t_1020 = ashr i64 %t64_1019, 6
  %fn_ret_1021 = call i64 @__omni_cyw_priv_b64_char(i64 %t_1020, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %fn_ret_1022 = call i64 @__omni_cyw_priv_w8(i64 %t_1018, i64 %fn_ret_1021, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_1023 = load i64, i64* %param_out_b64_731
  %t64_1024 = load i64, i64* %var_op_978
  %t_1025 = add i64 %t64_1023, %t64_1024
  %t_1026 = add i64 %t_1025, 3
  %t64_1027 = load i64, i64* %var_v24_999
  %fn_ret_1028 = call i64 @__omni_cyw_priv_b64_char(i64 %t64_1027, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %fn_ret_1029 = call i64 @__omni_cyw_priv_w8(i64 %t_1026, i64 %fn_ret_1028, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  call void @__omni_c_return_ptr(i8* null)
  %t64_1030 = load i64, i64* %var_gi_977
  %t_1031 = add i64 %t64_1030, 3
  store i64 %t_1031, i64* %var_gi_977
  call void @__omni_c_return_ptr(i8* null)
  %t64_1032 = load i64, i64* %var_op_978
  %t_1033 = add i64 %t64_1032, 4
  store i64 %t_1033, i64* %var_op_978
  br label %while_cond_33

while_exit_33:
  call void @__omni_c_return_ptr(i8* null)
  %t64_1034 = load i64, i64* %var_dig_742
  %t_1035 = add i64 %t64_1034, 18
  %fn_ret_1036 = call i64 @__omni_cyw_priv_r8(i64 %t_1035, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t_1037 = shl i64 %fn_ret_1036, 16
  %t64_1038 = load i64, i64* %var_dig_742
  %t_1039 = add i64 %t64_1038, 19
  %fn_ret_1040 = call i64 @__omni_cyw_priv_r8(i64 %t_1039, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t_1041 = shl i64 %fn_ret_1040, 8
  %t_1042 = or i64 %t_1037, %t_1041
  store i64 %t_1042, i64* %var_tail16_1043
  %t64_1044 = load i64, i64* %param_out_b64_731
  %t_1045 = add i64 %t64_1044, 24
  %t64_1046 = load i64, i64* %var_tail16_1043
  %t_1047 = ashr i64 %t64_1046, 18
  %fn_ret_1048 = call i64 @__omni_cyw_priv_b64_char(i64 %t_1047, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %fn_ret_1049 = call i64 @__omni_cyw_priv_w8(i64 %t_1045, i64 %fn_ret_1048, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_1050 = load i64, i64* %param_out_b64_731
  %t_1051 = add i64 %t64_1050, 25
  %t64_1052 = load i64, i64* %var_tail16_1043
  %t_1053 = ashr i64 %t64_1052, 12
  %fn_ret_1054 = call i64 @__omni_cyw_priv_b64_char(i64 %t_1053, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %fn_ret_1055 = call i64 @__omni_cyw_priv_w8(i64 %t_1051, i64 %fn_ret_1054, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_1056 = load i64, i64* %param_out_b64_731
  %t_1057 = add i64 %t64_1056, 26
  %t64_1058 = load i64, i64* %var_tail16_1043
  %t_1059 = ashr i64 %t64_1058, 6
  %fn_ret_1060 = call i64 @__omni_cyw_priv_b64_char(i64 %t_1059, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %fn_ret_1061 = call i64 @__omni_cyw_priv_w8(i64 %t_1057, i64 %fn_ret_1060, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_1062 = load i64, i64* %param_out_b64_731
  %t_1063 = add i64 %t64_1062, 27
  %fn_ret_1064 = call i64 @__omni_cyw_priv_w8(i64 %t_1063, i64 61, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_1065 = load i64, i64* %param_out_b64_731
  %t_1066 = add i64 %t64_1065, 28
  %fn_ret_1067 = call i64 @__omni_cyw_priv_w8(i64 %t_1066, i64 0, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  ret i64 28
}


define void @__omni_trampoline_cyw_ws_compute_accept(i8* %raw_arg) {
entry:
  %raw_int = ptrtoint i8* %raw_arg to i64
  %res = call i64 @__omni_fn_cyw_ws_compute_accept(i64 %raw_int, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  ret void
}

define void @__omni_trampoline_pack_cyw_ws_compute_accept(i8* %raw_pack) {
entry:
  %p64 = bitcast i8* %raw_pack to i64*
  %p0 = getelementptr inbounds i64, i64* %p64, i64 0
  %a0 = load i64, i64* %p0
  %p1 = getelementptr inbounds i64, i64* %p64, i64 1
  %a1 = load i64, i64* %p1
  %p2 = getelementptr inbounds i64, i64* %p64, i64 2
  %a2 = load i64, i64* %p2
  %p3 = getelementptr inbounds i64, i64* %p64, i64 3
  %a3 = load i64, i64* %p3
  %p4 = getelementptr inbounds i64, i64* %p64, i64 4
  %a4 = load i64, i64* %p4
  %p5 = getelementptr inbounds i64, i64* %p64, i64 5
  %a5 = load i64, i64* %p5
  call void @free(i8* %raw_pack)
  %res = call i64 @__omni_fn_cyw_ws_compute_accept(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5)
  ret void
}

define i64 @__omni_cyw_priv_sdpcm_next_seq(i64 %arg0, i64 %arg1, i64 %arg2, i64 %arg3, i64 %arg4, i64 %arg5) {
entry:
  %var_scr_1069 = alloca i64
  %var_s_1074 = alloca i64
  call void @__omni_c_return_ptr(i8* null)
  %ext_i64_1068 = call i64 @cyw_ll_sram_scratch()
  store i64 %ext_i64_1068, i64* %var_scr_1069
  call void @__omni_c_return_ptr(i8* null)
  %t64_1070 = load i64, i64* %var_scr_1069
  %t_1071 = add i64 %t64_1070, 4
  %ext_i64_1072 = call i64 @mmio_read32(i64 %t_1071)
  %t_1073 = and i64 %ext_i64_1072, 255
  store i64 %t_1073, i64* %var_s_1074
  %t64_1075 = load i64, i64* %var_scr_1069
  %t_1076 = add i64 %t64_1075, 4
  %t64_1077 = load i64, i64* %var_s_1074
  %t_1078 = add i64 %t64_1077, 1
  %t_1079 = and i64 %t_1078, 255
  %ext_i64_1080 = call i64 @mmio_write32(i64 %t_1076, i64 %t_1079)
  %t64_1081 = load i64, i64* %var_s_1074
  ret i64 %t64_1081
}


define void @__omni_trampoline_sdpcm_next_seq(i8* %raw_arg) {
entry:
  %raw_int = ptrtoint i8* %raw_arg to i64
  %res = call i64 @__omni_cyw_priv_sdpcm_next_seq(i64 %raw_int, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  ret void
}

define void @__omni_trampoline_pack_sdpcm_next_seq(i8* %raw_pack) {
entry:
  %p64 = bitcast i8* %raw_pack to i64*
  %p0 = getelementptr inbounds i64, i64* %p64, i64 0
  %a0 = load i64, i64* %p0
  %p1 = getelementptr inbounds i64, i64* %p64, i64 1
  %a1 = load i64, i64* %p1
  %p2 = getelementptr inbounds i64, i64* %p64, i64 2
  %a2 = load i64, i64* %p2
  %p3 = getelementptr inbounds i64, i64* %p64, i64 3
  %a3 = load i64, i64* %p3
  %p4 = getelementptr inbounds i64, i64* %p64, i64 4
  %a4 = load i64, i64* %p4
  %p5 = getelementptr inbounds i64, i64* %p64, i64 5
  %a5 = load i64, i64* %p5
  call void @free(i8* %raw_pack)
  %res = call i64 @__omni_cyw_priv_sdpcm_next_seq(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5)
  ret void
}

define i64 @__omni_cyw_priv_sdpcm_next_reqid(i64 %arg0, i64 %arg1, i64 %arg2, i64 %arg3, i64 %arg4, i64 %arg5) {
entry:
  %var_scr_1083 = alloca i64
  %var_r_1088 = alloca i64
  %var_nxt_1092 = alloca i64
  call void @__omni_c_return_ptr(i8* null)
  %ext_i64_1082 = call i64 @cyw_ll_sram_scratch()
  store i64 %ext_i64_1082, i64* %var_scr_1083
  call void @__omni_c_return_ptr(i8* null)
  %t64_1084 = load i64, i64* %var_scr_1083
  %t_1085 = add i64 %t64_1084, 8
  %ext_i64_1086 = call i64 @mmio_read32(i64 %t_1085)
  %t_1087 = and i64 %ext_i64_1086, 65535
  store i64 %t_1087, i64* %var_r_1088
  call void @__omni_c_return_ptr(i8* null)
  %t64_1089 = load i64, i64* %var_r_1088
  %t_1090 = add i64 %t64_1089, 1
  %t_1091 = and i64 %t_1090, 65535
  store i64 %t_1091, i64* %var_nxt_1092
  %t64_1093 = load i64, i64* %var_nxt_1092
  %cond_1094 = icmp eq i64 %t64_1093, 0
  br i1 %cond_1094, label %if_then_34, label %if_merge_34

if_then_34:
  call void @__omni_c_return_ptr(i8* null)
  store i64 1, i64* %var_nxt_1092
  br label %if_merge_34

if_merge_34:
  %t64_1095 = load i64, i64* %var_scr_1083
  %t_1096 = add i64 %t64_1095, 8
  %t64_1097 = load i64, i64* %var_nxt_1092
  %ext_i64_1098 = call i64 @mmio_write32(i64 %t_1096, i64 %t64_1097)
  %t64_1099 = load i64, i64* %var_r_1088
  ret i64 %t64_1099
}


define void @__omni_trampoline_sdpcm_next_reqid(i8* %raw_arg) {
entry:
  %raw_int = ptrtoint i8* %raw_arg to i64
  %res = call i64 @__omni_cyw_priv_sdpcm_next_reqid(i64 %raw_int, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  ret void
}

define void @__omni_trampoline_pack_sdpcm_next_reqid(i8* %raw_pack) {
entry:
  %p64 = bitcast i8* %raw_pack to i64*
  %p0 = getelementptr inbounds i64, i64* %p64, i64 0
  %a0 = load i64, i64* %p0
  %p1 = getelementptr inbounds i64, i64* %p64, i64 1
  %a1 = load i64, i64* %p1
  %p2 = getelementptr inbounds i64, i64* %p64, i64 2
  %a2 = load i64, i64* %p2
  %p3 = getelementptr inbounds i64, i64* %p64, i64 3
  %a3 = load i64, i64* %p3
  %p4 = getelementptr inbounds i64, i64* %p64, i64 4
  %a4 = load i64, i64* %p4
  %p5 = getelementptr inbounds i64, i64* %p64, i64 5
  %a5 = load i64, i64* %p5
  call void @free(i8* %raw_pack)
  %res = call i64 @__omni_cyw_priv_sdpcm_next_reqid(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5)
  ret void
}

define i64 @__omni_fn_cyw_f2_write(i64 %arg0, i64 %arg1, i64 %arg2, i64 %arg3, i64 %arg4, i64 %arg5) {
entry:
  %param_buf_ptr_1101 = alloca i64
  %param_byte_len_1103 = alloca i64
  %var_tries_1104 = alloca i64
  %var_st_1108 = alloca i64
  %var_aligned_1118 = alloca i64
  %var_i_1120 = alloca i64
  %var_cmd_1132 = alloca i64
  %param_unset_buf_ptr_1100 = icmp eq i64 %arg0, -581005069656919554
  %param_dsel_buf_ptr_1100 = select i1 %param_unset_buf_ptr_1100, i64 0, i64 %arg0
  store i64 %param_dsel_buf_ptr_1100, i64* %param_buf_ptr_1101
  %param_unset_byte_len_1102 = icmp eq i64 %arg1, -581005069656919554
  %param_dsel_byte_len_1102 = select i1 %param_unset_byte_len_1102, i64 0, i64 %arg1
  store i64 %param_dsel_byte_len_1102, i64* %param_byte_len_1103
  call void @__omni_c_return_ptr(i8* null)
  store i64 0, i64* %var_tries_1104
  br label %while_cond_35

while_cond_35:
  %t64_1105 = load i64, i64* %var_tries_1104
  %cond_1106 = icmp slt i64 %t64_1105, 40
  br i1 %cond_1106, label %while_body_35, label %while_exit_35

while_body_35:
  call void @__omni_c_return_ptr(i8* null)
  %ext_i64_1107 = call i64 @cyw_ll_read_reg(i64 0, i64 8, i64 4)
  store i64 %ext_i64_1107, i64* %var_st_1108
  %t64_1109 = load i64, i64* %var_st_1108
  %t_1110 = and i64 %t64_1109, 32
  %cond_1111 = icmp ne i64 %t_1110, 0
  br i1 %cond_1111, label %if_then_36, label %if_merge_36

if_then_36:
  br label %while_exit_35
if_merge_36:
  %ext_i64_1112 = call i64 @sleep_us(i64 250)
  call void @__omni_c_return_ptr(i8* null)
  %t64_1113 = load i64, i64* %var_tries_1104
  %t_1114 = add i64 %t64_1113, 1
  store i64 %t_1114, i64* %var_tries_1104
  br label %while_cond_35

while_exit_35:
  call void @__omni_c_return_ptr(i8* null)
  %t64_1115 = load i64, i64* %param_byte_len_1103
  %t_1116 = add i64 %t64_1115, 3
  %t_1117 = and i64 %t_1116, 65532
  store i64 %t_1117, i64* %var_aligned_1118
  call void @__omni_c_return_ptr(i8* null)
  %t64_1119 = load i64, i64* %param_byte_len_1103
  store i64 %t64_1119, i64* %var_i_1120
  br label %while_cond_37

while_cond_37:
  %t64_1121 = load i64, i64* %var_i_1120
  %t64_1122 = load i64, i64* %var_aligned_1118
  %cond_1123 = icmp slt i64 %t64_1121, %t64_1122
  br i1 %cond_1123, label %while_body_37, label %while_exit_37

while_body_37:
  %t64_1124 = load i64, i64* %param_buf_ptr_1101
  %t64_1125 = load i64, i64* %var_i_1120
  %t_1126 = add i64 %t64_1124, %t64_1125
  %fn_ret_1127 = call i64 @__omni_cyw_priv_w8(i64 %t_1126, i64 0, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  call void @__omni_c_return_ptr(i8* null)
  %t64_1128 = load i64, i64* %var_i_1120
  %t_1129 = add i64 %t64_1128, 1
  store i64 %t_1129, i64* %var_i_1120
  br label %while_cond_37

while_exit_37:
  call void @__omni_c_return_ptr(i8* null)
  %t64_1130 = load i64, i64* %var_aligned_1118
  %ext_i64_1131 = call i64 @cyw_ll_pack_cmd(i64 1, i64 1, i64 2, i64 0, i64 %t64_1130)
  store i64 %ext_i64_1131, i64* %var_cmd_1132
  %ext_i64_1133 = call i64 @gpio_put(i64 25, i64 0)
  %t64_1134 = load i64, i64* %var_cmd_1132
  %ext_i64_1135 = call i64 @cyw_ll_gspi_write_u32(i64 %t64_1134)
  %t64_1136 = load i64, i64* %param_buf_ptr_1101
  %t64_1137 = load i64, i64* %var_aligned_1118
  %ext_i64_1138 = call i64 @cyw_ll_gspi_write_bytes(i64 %t64_1136, i64 %t64_1137)
  %ext_i64_1139 = call i64 @gpio_put(i64 25, i64 1)
  %t64_1140 = load i64, i64* %var_aligned_1118
  ret i64 %t64_1140
}


define void @__omni_trampoline_cyw_f2_write(i8* %raw_arg) {
entry:
  %raw_int = ptrtoint i8* %raw_arg to i64
  %res = call i64 @__omni_fn_cyw_f2_write(i64 %raw_int, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  ret void
}

define void @__omni_trampoline_pack_cyw_f2_write(i8* %raw_pack) {
entry:
  %p64 = bitcast i8* %raw_pack to i64*
  %p0 = getelementptr inbounds i64, i64* %p64, i64 0
  %a0 = load i64, i64* %p0
  %p1 = getelementptr inbounds i64, i64* %p64, i64 1
  %a1 = load i64, i64* %p1
  %p2 = getelementptr inbounds i64, i64* %p64, i64 2
  %a2 = load i64, i64* %p2
  %p3 = getelementptr inbounds i64, i64* %p64, i64 3
  %a3 = load i64, i64* %p3
  %p4 = getelementptr inbounds i64, i64* %p64, i64 4
  %a4 = load i64, i64* %p4
  %p5 = getelementptr inbounds i64, i64* %p64, i64 5
  %a5 = load i64, i64* %p5
  call void @free(i8* %raw_pack)
  %res = call i64 @__omni_fn_cyw_f2_write(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5)
  ret void
}

define i64 @__omni_fn_cyw_tx_eth(i64 %arg0, i64 %arg1, i64 %arg2, i64 %arg3, i64 %arg4, i64 %arg5) {
entry:
  %param_ctx_1142 = alloca i64
  %param_eth_ptr_1144 = alloca i64
  %param_eth_len_1146 = alloca i64
  %var_tx_1149 = alloca i64
  %var_sdpcm_len_1152 = alloca i64
  %param_unset_ctx_1141 = icmp eq i64 %arg0, -581005069656919554
  %param_dsel_ctx_1141 = select i1 %param_unset_ctx_1141, i64 0, i64 %arg0
  store i64 %param_dsel_ctx_1141, i64* %param_ctx_1142
  %param_unset_eth_ptr_1143 = icmp eq i64 %arg1, -581005069656919554
  %param_dsel_eth_ptr_1143 = select i1 %param_unset_eth_ptr_1143, i64 0, i64 %arg1
  store i64 %param_dsel_eth_ptr_1143, i64* %param_eth_ptr_1144
  %param_unset_eth_len_1145 = icmp eq i64 %arg2, -581005069656919554
  %param_dsel_eth_len_1145 = select i1 %param_unset_eth_len_1145, i64 0, i64 %arg2
  store i64 %param_dsel_eth_len_1145, i64* %param_eth_len_1146
  call void @__omni_c_return_ptr(i8* null)
  %t64_1147 = load i64, i64* %param_ctx_1142
  %t_1148 = add i64 %t64_1147, 4672
  store i64 %t_1148, i64* %var_tx_1149
  call void @__omni_c_return_ptr(i8* null)
  %t64_1150 = load i64, i64* %param_eth_len_1146
  %t_1151 = add i64 18, %t64_1150
  store i64 %t_1151, i64* %var_sdpcm_len_1152
  %t64_1153 = load i64, i64* %param_eth_ptr_1144
  %t64_1154 = load i64, i64* %var_tx_1149
  %t_1155 = add i64 %t64_1154, 18
  %cond_1156 = icmp ne i64 %t64_1153, %t_1155
  br i1 %cond_1156, label %if_then_38, label %if_merge_38

if_then_38:
  %t64_1157 = load i64, i64* %var_tx_1149
  %t_1158 = add i64 %t64_1157, 18
  %t64_1159 = load i64, i64* %param_eth_ptr_1144
  %t64_1160 = load i64, i64* %param_eth_len_1146
  %fn_ret_1161 = call i64 @__omni_cyw_priv_bcopy(i64 %t_1158, i64 %t64_1159, i64 %t64_1160, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  br label %if_merge_38

if_merge_38:
  %t64_1162 = load i64, i64* %var_tx_1149
  %t_1163 = add i64 %t64_1162, 0
  %t64_1164 = load i64, i64* %var_sdpcm_len_1152
  %fn_ret_1165 = call i64 @__omni_cyw_priv_w16_le(i64 %t_1163, i64 %t64_1164, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_1166 = load i64, i64* %var_tx_1149
  %t_1167 = add i64 %t64_1166, 2
  %t64_1168 = load i64, i64* %var_sdpcm_len_1152
  %t_1169 = xor i64 %t64_1168, 65535
  %t_1170 = and i64 %t_1169, 65535
  %fn_ret_1171 = call i64 @__omni_cyw_priv_w16_le(i64 %t_1167, i64 %t_1170, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_1172 = load i64, i64* %var_tx_1149
  %t_1173 = add i64 %t64_1172, 4
  %fn_ret_1174 = call i64 @__omni_cyw_priv_sdpcm_next_seq(i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %fn_ret_1175 = call i64 @__omni_cyw_priv_w8(i64 %t_1173, i64 %fn_ret_1174, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_1176 = load i64, i64* %var_tx_1149
  %t_1177 = add i64 %t64_1176, 5
  %fn_ret_1178 = call i64 @__omni_cyw_priv_w8(i64 %t_1177, i64 2, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_1179 = load i64, i64* %var_tx_1149
  %t_1180 = add i64 %t64_1179, 6
  %fn_ret_1181 = call i64 @__omni_cyw_priv_w8(i64 %t_1180, i64 0, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_1182 = load i64, i64* %var_tx_1149
  %t_1183 = add i64 %t64_1182, 7
  %fn_ret_1184 = call i64 @__omni_cyw_priv_w8(i64 %t_1183, i64 14, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_1185 = load i64, i64* %var_tx_1149
  %t_1186 = add i64 %t64_1185, 8
  %fn_ret_1187 = call i64 @__omni_cyw_priv_w32_le(i64 %t_1186, i64 0, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_1188 = load i64, i64* %var_tx_1149
  %t_1189 = add i64 %t64_1188, 12
  %fn_ret_1190 = call i64 @__omni_cyw_priv_w16_le(i64 %t_1189, i64 0, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_1191 = load i64, i64* %var_tx_1149
  %t_1192 = add i64 %t64_1191, 14
  %fn_ret_1193 = call i64 @__omni_cyw_priv_w8(i64 %t_1192, i64 32, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_1194 = load i64, i64* %var_tx_1149
  %t_1195 = add i64 %t64_1194, 15
  %fn_ret_1196 = call i64 @__omni_cyw_priv_w8(i64 %t_1195, i64 0, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_1197 = load i64, i64* %var_tx_1149
  %t_1198 = add i64 %t64_1197, 16
  %fn_ret_1199 = call i64 @__omni_cyw_priv_w8(i64 %t_1198, i64 0, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_1200 = load i64, i64* %var_tx_1149
  %t_1201 = add i64 %t64_1200, 17
  %fn_ret_1202 = call i64 @__omni_cyw_priv_w8(i64 %t_1201, i64 0, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_1203 = load i64, i64* %var_tx_1149
  %t64_1204 = load i64, i64* %var_sdpcm_len_1152
  %fn_ret_1205 = call i64 @__omni_fn_cyw_f2_write(i64 %t64_1203, i64 %t64_1204, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  ret i64 %fn_ret_1205
}


define void @__omni_trampoline_cyw_tx_eth(i8* %raw_arg) {
entry:
  %raw_int = ptrtoint i8* %raw_arg to i64
  %res = call i64 @__omni_fn_cyw_tx_eth(i64 %raw_int, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  ret void
}

define void @__omni_trampoline_pack_cyw_tx_eth(i8* %raw_pack) {
entry:
  %p64 = bitcast i8* %raw_pack to i64*
  %p0 = getelementptr inbounds i64, i64* %p64, i64 0
  %a0 = load i64, i64* %p0
  %p1 = getelementptr inbounds i64, i64* %p64, i64 1
  %a1 = load i64, i64* %p1
  %p2 = getelementptr inbounds i64, i64* %p64, i64 2
  %a2 = load i64, i64* %p2
  %p3 = getelementptr inbounds i64, i64* %p64, i64 3
  %a3 = load i64, i64* %p3
  %p4 = getelementptr inbounds i64, i64* %p64, i64 4
  %a4 = load i64, i64* %p4
  %p5 = getelementptr inbounds i64, i64* %p64, i64 5
  %a5 = load i64, i64* %p5
  call void @free(i8* %raw_pack)
  %res = call i64 @__omni_fn_cyw_tx_eth(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5)
  ret void
}

define i64 @__omni_cyw_priv_build_eth_hdr(i64 %arg0, i64 %arg1, i64 %arg2, i64 %arg3, i64 %arg4, i64 %arg5) {
entry:
  %param_buf_1207 = alloca i64
  %param_dmac_1209 = alloca i64
  %param_smac_1211 = alloca i64
  %param_etype_1213 = alloca i64
  %param_unset_buf_1206 = icmp eq i64 %arg0, -581005069656919554
  %param_dsel_buf_1206 = select i1 %param_unset_buf_1206, i64 0, i64 %arg0
  store i64 %param_dsel_buf_1206, i64* %param_buf_1207
  %param_unset_dmac_1208 = icmp eq i64 %arg1, -581005069656919554
  %param_dsel_dmac_1208 = select i1 %param_unset_dmac_1208, i64 0, i64 %arg1
  store i64 %param_dsel_dmac_1208, i64* %param_dmac_1209
  %param_unset_smac_1210 = icmp eq i64 %arg2, -581005069656919554
  %param_dsel_smac_1210 = select i1 %param_unset_smac_1210, i64 0, i64 %arg2
  store i64 %param_dsel_smac_1210, i64* %param_smac_1211
  %param_unset_etype_1212 = icmp eq i64 %arg3, -581005069656919554
  %param_dsel_etype_1212 = select i1 %param_unset_etype_1212, i64 0, i64 %arg3
  store i64 %param_dsel_etype_1212, i64* %param_etype_1213
  %t64_1214 = load i64, i64* %param_buf_1207
  %t64_1215 = load i64, i64* %param_dmac_1209
  %fn_ret_1216 = call i64 @__omni_cyw_priv_bcopy(i64 %t64_1214, i64 %t64_1215, i64 6, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_1217 = load i64, i64* %param_buf_1207
  %t_1218 = add i64 %t64_1217, 6
  %t64_1219 = load i64, i64* %param_smac_1211
  %fn_ret_1220 = call i64 @__omni_cyw_priv_bcopy(i64 %t_1218, i64 %t64_1219, i64 6, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_1221 = load i64, i64* %param_buf_1207
  %t_1222 = add i64 %t64_1221, 12
  %t64_1223 = load i64, i64* %param_etype_1213
  %fn_ret_1224 = call i64 @__omni_cyw_priv_w16_be(i64 %t_1222, i64 %t64_1223, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  ret i64 14
}


define void @__omni_trampoline_build_eth_hdr(i8* %raw_arg) {
entry:
  %raw_int = ptrtoint i8* %raw_arg to i64
  %res = call i64 @__omni_cyw_priv_build_eth_hdr(i64 %raw_int, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  ret void
}

define void @__omni_trampoline_pack_build_eth_hdr(i8* %raw_pack) {
entry:
  %p64 = bitcast i8* %raw_pack to i64*
  %p0 = getelementptr inbounds i64, i64* %p64, i64 0
  %a0 = load i64, i64* %p0
  %p1 = getelementptr inbounds i64, i64* %p64, i64 1
  %a1 = load i64, i64* %p1
  %p2 = getelementptr inbounds i64, i64* %p64, i64 2
  %a2 = load i64, i64* %p2
  %p3 = getelementptr inbounds i64, i64* %p64, i64 3
  %a3 = load i64, i64* %p3
  %p4 = getelementptr inbounds i64, i64* %p64, i64 4
  %a4 = load i64, i64* %p4
  %p5 = getelementptr inbounds i64, i64* %p64, i64 5
  %a5 = load i64, i64* %p5
  call void @free(i8* %raw_pack)
  %res = call i64 @__omni_cyw_priv_build_eth_hdr(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5)
  ret void
}

define i64 @__omni_cyw_priv_build_ipv4_hdr(i64 %arg0, i64 %arg1, i64 %arg2, i64 %arg3, i64 %arg4, i64 %arg5) {
entry:
  %param_ctx_1226 = alloca i64
  %param_buf_1228 = alloca i64
  %param_sip_1230 = alloca i64
  %param_dip_1232 = alloca i64
  %param_proto_1234 = alloca i64
  %param_payload_len_1236 = alloca i64
  %var_ip_id_1242 = alloca i64
  %var_csum_1287 = alloca i64
  %param_unset_ctx_1225 = icmp eq i64 %arg0, -581005069656919554
  %param_dsel_ctx_1225 = select i1 %param_unset_ctx_1225, i64 0, i64 %arg0
  store i64 %param_dsel_ctx_1225, i64* %param_ctx_1226
  %param_unset_buf_1227 = icmp eq i64 %arg1, -581005069656919554
  %param_dsel_buf_1227 = select i1 %param_unset_buf_1227, i64 0, i64 %arg1
  store i64 %param_dsel_buf_1227, i64* %param_buf_1228
  %param_unset_sip_1229 = icmp eq i64 %arg2, -581005069656919554
  %param_dsel_sip_1229 = select i1 %param_unset_sip_1229, i64 0, i64 %arg2
  store i64 %param_dsel_sip_1229, i64* %param_sip_1230
  %param_unset_dip_1231 = icmp eq i64 %arg3, -581005069656919554
  %param_dsel_dip_1231 = select i1 %param_unset_dip_1231, i64 0, i64 %arg3
  store i64 %param_dsel_dip_1231, i64* %param_dip_1232
  %param_unset_proto_1233 = icmp eq i64 %arg4, -581005069656919554
  %param_dsel_proto_1233 = select i1 %param_unset_proto_1233, i64 0, i64 %arg4
  store i64 %param_dsel_proto_1233, i64* %param_proto_1234
  %param_unset_payload_len_1235 = icmp eq i64 %arg5, -581005069656919554
  %param_dsel_payload_len_1235 = select i1 %param_unset_payload_len_1235, i64 0, i64 %arg5
  store i64 %param_dsel_payload_len_1235, i64* %param_payload_len_1236
  call void @__omni_c_return_ptr(i8* null)
  %t64_1237 = load i64, i64* %param_ctx_1226
  %t_1238 = add i64 %t64_1237, 20
  %ext_i64_1239 = call i64 @mmio_read32(i64 %t_1238)
  %t_1240 = add i64 %ext_i64_1239, 1
  %t_1241 = and i64 %t_1240, 65535
  store i64 %t_1241, i64* %var_ip_id_1242
  %t64_1243 = load i64, i64* %param_ctx_1226
  %t_1244 = add i64 %t64_1243, 20
  %t64_1245 = load i64, i64* %var_ip_id_1242
  %ext_i64_1246 = call i64 @mmio_write32(i64 %t_1244, i64 %t64_1245)
  %t64_1247 = load i64, i64* %param_buf_1228
  %t_1248 = add i64 %t64_1247, 0
  %fn_ret_1249 = call i64 @__omni_cyw_priv_w8(i64 %t_1248, i64 69, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_1250 = load i64, i64* %param_buf_1228
  %t_1251 = add i64 %t64_1250, 1
  %fn_ret_1252 = call i64 @__omni_cyw_priv_w8(i64 %t_1251, i64 0, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_1253 = load i64, i64* %param_buf_1228
  %t_1254 = add i64 %t64_1253, 2
  %t64_1255 = load i64, i64* %param_payload_len_1236
  %t_1256 = add i64 20, %t64_1255
  %fn_ret_1257 = call i64 @__omni_cyw_priv_w16_be(i64 %t_1254, i64 %t_1256, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_1258 = load i64, i64* %param_buf_1228
  %t_1259 = add i64 %t64_1258, 4
  %t64_1260 = load i64, i64* %var_ip_id_1242
  %fn_ret_1261 = call i64 @__omni_cyw_priv_w16_be(i64 %t_1259, i64 %t64_1260, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_1262 = load i64, i64* %param_buf_1228
  %t_1263 = add i64 %t64_1262, 6
  %fn_ret_1264 = call i64 @__omni_cyw_priv_w16_be(i64 %t_1263, i64 0, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_1265 = load i64, i64* %param_buf_1228
  %t_1266 = add i64 %t64_1265, 8
  %fn_ret_1267 = call i64 @__omni_cyw_priv_w8(i64 %t_1266, i64 64, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_1268 = load i64, i64* %param_buf_1228
  %t_1269 = add i64 %t64_1268, 9
  %t64_1270 = load i64, i64* %param_proto_1234
  %t_1271 = and i64 %t64_1270, 255
  %fn_ret_1272 = call i64 @__omni_cyw_priv_w8(i64 %t_1269, i64 %t_1271, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_1273 = load i64, i64* %param_buf_1228
  %t_1274 = add i64 %t64_1273, 10
  %fn_ret_1275 = call i64 @__omni_cyw_priv_w16_be(i64 %t_1274, i64 0, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_1276 = load i64, i64* %param_buf_1228
  %t_1277 = add i64 %t64_1276, 12
  %t64_1278 = load i64, i64* %param_sip_1230
  %fn_ret_1279 = call i64 @__omni_cyw_priv_bcopy(i64 %t_1277, i64 %t64_1278, i64 4, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_1280 = load i64, i64* %param_buf_1228
  %t_1281 = add i64 %t64_1280, 16
  %t64_1282 = load i64, i64* %param_dip_1232
  %fn_ret_1283 = call i64 @__omni_cyw_priv_bcopy(i64 %t_1281, i64 %t64_1282, i64 4, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  call void @__omni_c_return_ptr(i8* null)
  %t64_1284 = load i64, i64* %param_buf_1228
  %fn_ret_1285 = call i64 @__omni_cyw_priv_inet_csum_acc(i64 0, i64 %t64_1284, i64 20, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %fn_ret_1286 = call i64 @__omni_cyw_priv_inet_csum_finish(i64 %fn_ret_1285, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  store i64 %fn_ret_1286, i64* %var_csum_1287
  %t64_1288 = load i64, i64* %param_buf_1228
  %t_1289 = add i64 %t64_1288, 10
  %t64_1290 = load i64, i64* %var_csum_1287
  %fn_ret_1291 = call i64 @__omni_cyw_priv_w16_be(i64 %t_1289, i64 %t64_1290, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  ret i64 20
}


define void @__omni_trampoline_build_ipv4_hdr(i8* %raw_arg) {
entry:
  %raw_int = ptrtoint i8* %raw_arg to i64
  %res = call i64 @__omni_cyw_priv_build_ipv4_hdr(i64 %raw_int, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  ret void
}

define void @__omni_trampoline_pack_build_ipv4_hdr(i8* %raw_pack) {
entry:
  %p64 = bitcast i8* %raw_pack to i64*
  %p0 = getelementptr inbounds i64, i64* %p64, i64 0
  %a0 = load i64, i64* %p0
  %p1 = getelementptr inbounds i64, i64* %p64, i64 1
  %a1 = load i64, i64* %p1
  %p2 = getelementptr inbounds i64, i64* %p64, i64 2
  %a2 = load i64, i64* %p2
  %p3 = getelementptr inbounds i64, i64* %p64, i64 3
  %a3 = load i64, i64* %p3
  %p4 = getelementptr inbounds i64, i64* %p64, i64 4
  %a4 = load i64, i64* %p4
  %p5 = getelementptr inbounds i64, i64* %p64, i64 5
  %a5 = load i64, i64* %p5
  call void @free(i8* %raw_pack)
  %res = call i64 @__omni_cyw_priv_build_ipv4_hdr(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5)
  ret void
}

define i64 @__omni_cyw_priv_send_arp_pkt(i64 %arg0, i64 %arg1, i64 %arg2, i64 %arg3, i64 %arg4, i64 %arg5) {
entry:
  %param_ctx_1293 = alloca i64
  %param_dst_mac_1295 = alloca i64
  %param_target_ip_1297 = alloca i64
  %param_op_1299 = alloca i64
  %var_pkt_1303 = alloca i64
  %var_my_mac_1306 = alloca i64
  %var_my_ip_1309 = alloca i64
  %param_unset_ctx_1292 = icmp eq i64 %arg0, -581005069656919554
  %param_dsel_ctx_1292 = select i1 %param_unset_ctx_1292, i64 0, i64 %arg0
  store i64 %param_dsel_ctx_1292, i64* %param_ctx_1293
  %param_unset_dst_mac_1294 = icmp eq i64 %arg1, -581005069656919554
  %param_dsel_dst_mac_1294 = select i1 %param_unset_dst_mac_1294, i64 0, i64 %arg1
  store i64 %param_dsel_dst_mac_1294, i64* %param_dst_mac_1295
  %param_unset_target_ip_1296 = icmp eq i64 %arg2, -581005069656919554
  %param_dsel_target_ip_1296 = select i1 %param_unset_target_ip_1296, i64 0, i64 %arg2
  store i64 %param_dsel_target_ip_1296, i64* %param_target_ip_1297
  %param_unset_op_1298 = icmp eq i64 %arg3, -581005069656919554
  %param_dsel_op_1298 = select i1 %param_unset_op_1298, i64 0, i64 %arg3
  store i64 %param_dsel_op_1298, i64* %param_op_1299
  call void @__omni_c_return_ptr(i8* null)
  %t64_1300 = load i64, i64* %param_ctx_1293
  %t_1301 = add i64 %t64_1300, 4672
  %t_1302 = add i64 %t_1301, 18
  store i64 %t_1302, i64* %var_pkt_1303
  call void @__omni_c_return_ptr(i8* null)
  %t64_1304 = load i64, i64* %param_ctx_1293
  %t_1305 = add i64 %t64_1304, 32
  store i64 %t_1305, i64* %var_my_mac_1306
  call void @__omni_c_return_ptr(i8* null)
  %t64_1307 = load i64, i64* %param_ctx_1293
  %t_1308 = add i64 %t64_1307, 40
  store i64 %t_1308, i64* %var_my_ip_1309
  %t64_1310 = load i64, i64* %var_pkt_1303
  %t64_1311 = load i64, i64* %param_dst_mac_1295
  %fn_ret_1312 = call i64 @__omni_cyw_priv_bcopy(i64 %t64_1310, i64 %t64_1311, i64 6, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_1313 = load i64, i64* %var_pkt_1303
  %t_1314 = add i64 %t64_1313, 6
  %t64_1315 = load i64, i64* %var_my_mac_1306
  %fn_ret_1316 = call i64 @__omni_cyw_priv_bcopy(i64 %t_1314, i64 %t64_1315, i64 6, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_1317 = load i64, i64* %var_pkt_1303
  %t_1318 = add i64 %t64_1317, 12
  %fn_ret_1319 = call i64 @__omni_cyw_priv_w16_be(i64 %t_1318, i64 2054, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_1320 = load i64, i64* %var_pkt_1303
  %t_1321 = add i64 %t64_1320, 14
  %fn_ret_1322 = call i64 @__omni_cyw_priv_w16_be(i64 %t_1321, i64 1, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_1323 = load i64, i64* %var_pkt_1303
  %t_1324 = add i64 %t64_1323, 16
  %fn_ret_1325 = call i64 @__omni_cyw_priv_w16_be(i64 %t_1324, i64 2048, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_1326 = load i64, i64* %var_pkt_1303
  %t_1327 = add i64 %t64_1326, 18
  %fn_ret_1328 = call i64 @__omni_cyw_priv_w8(i64 %t_1327, i64 6, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_1329 = load i64, i64* %var_pkt_1303
  %t_1330 = add i64 %t64_1329, 19
  %fn_ret_1331 = call i64 @__omni_cyw_priv_w8(i64 %t_1330, i64 4, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_1332 = load i64, i64* %var_pkt_1303
  %t_1333 = add i64 %t64_1332, 20
  %t64_1334 = load i64, i64* %param_op_1299
  %fn_ret_1335 = call i64 @__omni_cyw_priv_w16_be(i64 %t_1333, i64 %t64_1334, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_1336 = load i64, i64* %var_pkt_1303
  %t_1337 = add i64 %t64_1336, 22
  %t64_1338 = load i64, i64* %var_my_mac_1306
  %fn_ret_1339 = call i64 @__omni_cyw_priv_bcopy(i64 %t_1337, i64 %t64_1338, i64 6, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_1340 = load i64, i64* %var_pkt_1303
  %t_1341 = add i64 %t64_1340, 28
  %t64_1342 = load i64, i64* %var_my_ip_1309
  %fn_ret_1343 = call i64 @__omni_cyw_priv_bcopy(i64 %t_1341, i64 %t64_1342, i64 4, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_1344 = load i64, i64* %param_op_1299
  %cond_1345 = icmp eq i64 %t64_1344, 1
  br i1 %cond_1345, label %if_then_39, label %if_else_39

if_then_39:
  %t64_1346 = load i64, i64* %var_pkt_1303
  %t_1347 = add i64 %t64_1346, 32
  %fn_ret_1348 = call i64 @__omni_cyw_priv_bzero(i64 %t_1347, i64 6, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  br label %if_merge_39

if_else_39:
  %t64_1349 = load i64, i64* %var_pkt_1303
  %t_1350 = add i64 %t64_1349, 32
  %t64_1351 = load i64, i64* %param_dst_mac_1295
  %fn_ret_1352 = call i64 @__omni_cyw_priv_bcopy(i64 %t_1350, i64 %t64_1351, i64 6, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  br label %if_merge_39

if_merge_39:
  %t64_1353 = load i64, i64* %var_pkt_1303
  %t_1354 = add i64 %t64_1353, 38
  %t64_1355 = load i64, i64* %param_target_ip_1297
  %fn_ret_1356 = call i64 @__omni_cyw_priv_bcopy(i64 %t_1354, i64 %t64_1355, i64 4, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_1357 = load i64, i64* %param_ctx_1293
  %t64_1358 = load i64, i64* %var_pkt_1303
  %fn_ret_1359 = call i64 @__omni_fn_cyw_tx_eth(i64 %t64_1357, i64 %t64_1358, i64 42, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  ret i64 %fn_ret_1359
}


define void @__omni_trampoline_send_arp_pkt(i8* %raw_arg) {
entry:
  %raw_int = ptrtoint i8* %raw_arg to i64
  %res = call i64 @__omni_cyw_priv_send_arp_pkt(i64 %raw_int, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  ret void
}

define void @__omni_trampoline_pack_send_arp_pkt(i8* %raw_pack) {
entry:
  %p64 = bitcast i8* %raw_pack to i64*
  %p0 = getelementptr inbounds i64, i64* %p64, i64 0
  %a0 = load i64, i64* %p0
  %p1 = getelementptr inbounds i64, i64* %p64, i64 1
  %a1 = load i64, i64* %p1
  %p2 = getelementptr inbounds i64, i64* %p64, i64 2
  %a2 = load i64, i64* %p2
  %p3 = getelementptr inbounds i64, i64* %p64, i64 3
  %a3 = load i64, i64* %p3
  %p4 = getelementptr inbounds i64, i64* %p64, i64 4
  %a4 = load i64, i64* %p4
  %p5 = getelementptr inbounds i64, i64* %p64, i64 5
  %a5 = load i64, i64* %p5
  call void @free(i8* %raw_pack)
  %res = call i64 @__omni_cyw_priv_send_arp_pkt(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5)
  ret void
}

define i64 @__omni_cyw_priv_send_udp_pkt(i64 %arg0, i64 %arg1, i64 %arg2, i64 %arg3, i64 %arg4, i64 %arg5, i64 %arg6, i64 %arg7) {
entry:
  %param_ctx_1361 = alloca i64
  %param_dmac_1363 = alloca i64
  %param_sip_1365 = alloca i64
  %param_dip_1367 = alloca i64
  %param_sport_1369 = alloca i64
  %param_dport_1371 = alloca i64
  %param_payload_ptr_1373 = alloca i64
  %param_payload_len_1375 = alloca i64
  %var_pkt_1379 = alloca i64
  %var_my_mac_1382 = alloca i64
  %var_udp_1410 = alloca i64
  %var_csum_1433 = alloca i64
  %param_unset_ctx_1360 = icmp eq i64 %arg0, -581005069656919554
  %param_dsel_ctx_1360 = select i1 %param_unset_ctx_1360, i64 0, i64 %arg0
  store i64 %param_dsel_ctx_1360, i64* %param_ctx_1361
  %param_unset_dmac_1362 = icmp eq i64 %arg1, -581005069656919554
  %param_dsel_dmac_1362 = select i1 %param_unset_dmac_1362, i64 0, i64 %arg1
  store i64 %param_dsel_dmac_1362, i64* %param_dmac_1363
  %param_unset_sip_1364 = icmp eq i64 %arg2, -581005069656919554
  %param_dsel_sip_1364 = select i1 %param_unset_sip_1364, i64 0, i64 %arg2
  store i64 %param_dsel_sip_1364, i64* %param_sip_1365
  %param_unset_dip_1366 = icmp eq i64 %arg3, -581005069656919554
  %param_dsel_dip_1366 = select i1 %param_unset_dip_1366, i64 0, i64 %arg3
  store i64 %param_dsel_dip_1366, i64* %param_dip_1367
  %param_unset_sport_1368 = icmp eq i64 %arg4, -581005069656919554
  %param_dsel_sport_1368 = select i1 %param_unset_sport_1368, i64 0, i64 %arg4
  store i64 %param_dsel_sport_1368, i64* %param_sport_1369
  %param_unset_dport_1370 = icmp eq i64 %arg5, -581005069656919554
  %param_dsel_dport_1370 = select i1 %param_unset_dport_1370, i64 0, i64 %arg5
  store i64 %param_dsel_dport_1370, i64* %param_dport_1371
  %param_unset_payload_ptr_1372 = icmp eq i64 %arg6, -581005069656919554
  %param_dsel_payload_ptr_1372 = select i1 %param_unset_payload_ptr_1372, i64 0, i64 %arg6
  store i64 %param_dsel_payload_ptr_1372, i64* %param_payload_ptr_1373
  %param_unset_payload_len_1374 = icmp eq i64 %arg7, -581005069656919554
  %param_dsel_payload_len_1374 = select i1 %param_unset_payload_len_1374, i64 0, i64 %arg7
  store i64 %param_dsel_payload_len_1374, i64* %param_payload_len_1375
  call void @__omni_c_return_ptr(i8* null)
  %t64_1376 = load i64, i64* %param_ctx_1361
  %t_1377 = add i64 %t64_1376, 4672
  %t_1378 = add i64 %t_1377, 18
  store i64 %t_1378, i64* %var_pkt_1379
  call void @__omni_c_return_ptr(i8* null)
  %t64_1380 = load i64, i64* %param_ctx_1361
  %t_1381 = add i64 %t64_1380, 32
  store i64 %t_1381, i64* %var_my_mac_1382
  %t64_1384 = load i64, i64* %param_payload_ptr_1373
  %cond_1385 = icmp ne i64 %t64_1384, 0
  br i1 %cond_1385, label %sc_and_rhs_1383, label %sc_and_short_1383

sc_and_short_1383:
  br label %sc_and_merge_1383

sc_and_rhs_1383:
  %t64_1386 = load i64, i64* %param_payload_ptr_1373
  %t64_1387 = load i64, i64* %var_pkt_1379
  %t_1388 = add i64 %t64_1387, 42
  %cond_1389 = icmp ne i64 %t64_1386, %t_1388
  br label %sc_and_done_1383

sc_and_done_1383:
  br label %sc_and_merge_1383

sc_and_merge_1383:
  %and_cond_1390 = phi i1 [ false, %sc_and_short_1383 ], [ %cond_1389, %sc_and_done_1383 ]
  br i1 %and_cond_1390, label %if_then_40, label %if_merge_40

if_then_40:
  %t64_1391 = load i64, i64* %var_pkt_1379
  %t_1392 = add i64 %t64_1391, 42
  %t64_1393 = load i64, i64* %param_payload_ptr_1373
  %t64_1394 = load i64, i64* %param_payload_len_1375
  %fn_ret_1395 = call i64 @__omni_cyw_priv_bcopy(i64 %t_1392, i64 %t64_1393, i64 %t64_1394, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  br label %if_merge_40

if_merge_40:
  %t64_1396 = load i64, i64* %var_pkt_1379
  %t64_1397 = load i64, i64* %param_dmac_1363
  %t64_1398 = load i64, i64* %var_my_mac_1382
  %fn_ret_1399 = call i64 @__omni_cyw_priv_build_eth_hdr(i64 %t64_1396, i64 %t64_1397, i64 %t64_1398, i64 2048, i64 -581005069656919554, i64 -581005069656919554)
  %t64_1400 = load i64, i64* %param_ctx_1361
  %t64_1401 = load i64, i64* %var_pkt_1379
  %t_1402 = add i64 %t64_1401, 14
  %t64_1403 = load i64, i64* %param_sip_1365
  %t64_1404 = load i64, i64* %param_dip_1367
  %t64_1405 = load i64, i64* %param_payload_len_1375
  %t_1406 = add i64 8, %t64_1405
  %fn_ret_1407 = call i64 @__omni_cyw_priv_build_ipv4_hdr(i64 %t64_1400, i64 %t_1402, i64 %t64_1403, i64 %t64_1404, i64 17, i64 %t_1406)
  call void @__omni_c_return_ptr(i8* null)
  %t64_1408 = load i64, i64* %var_pkt_1379
  %t_1409 = add i64 %t64_1408, 34
  store i64 %t_1409, i64* %var_udp_1410
  %t64_1411 = load i64, i64* %var_udp_1410
  %t_1412 = add i64 %t64_1411, 0
  %t64_1413 = load i64, i64* %param_sport_1369
  %fn_ret_1414 = call i64 @__omni_cyw_priv_w16_be(i64 %t_1412, i64 %t64_1413, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_1415 = load i64, i64* %var_udp_1410
  %t_1416 = add i64 %t64_1415, 2
  %t64_1417 = load i64, i64* %param_dport_1371
  %fn_ret_1418 = call i64 @__omni_cyw_priv_w16_be(i64 %t_1416, i64 %t64_1417, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_1419 = load i64, i64* %var_udp_1410
  %t_1420 = add i64 %t64_1419, 4
  %t64_1421 = load i64, i64* %param_payload_len_1375
  %t_1422 = add i64 8, %t64_1421
  %fn_ret_1423 = call i64 @__omni_cyw_priv_w16_be(i64 %t_1420, i64 %t_1422, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_1424 = load i64, i64* %var_udp_1410
  %t_1425 = add i64 %t64_1424, 6
  %fn_ret_1426 = call i64 @__omni_cyw_priv_w16_be(i64 %t_1425, i64 0, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  call void @__omni_c_return_ptr(i8* null)
  %t64_1427 = load i64, i64* %param_sip_1365
  %t64_1428 = load i64, i64* %param_dip_1367
  %t64_1429 = load i64, i64* %var_udp_1410
  %t64_1430 = load i64, i64* %param_payload_len_1375
  %t_1431 = add i64 8, %t64_1430
  %fn_ret_1432 = call i64 @__omni_cyw_priv_l4_checksum(i64 %t64_1427, i64 %t64_1428, i64 17, i64 %t64_1429, i64 %t_1431, i64 -581005069656919554)
  store i64 %fn_ret_1432, i64* %var_csum_1433
  %t64_1434 = load i64, i64* %var_csum_1433
  %cond_1435 = icmp eq i64 %t64_1434, 0
  br i1 %cond_1435, label %if_then_41, label %if_merge_41

if_then_41:
  call void @__omni_c_return_ptr(i8* null)
  store i64 65535, i64* %var_csum_1433
  br label %if_merge_41

if_merge_41:
  %t64_1436 = load i64, i64* %var_udp_1410
  %t_1437 = add i64 %t64_1436, 6
  %t64_1438 = load i64, i64* %var_csum_1433
  %fn_ret_1439 = call i64 @__omni_cyw_priv_w16_be(i64 %t_1437, i64 %t64_1438, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_1440 = load i64, i64* %param_ctx_1361
  %t64_1441 = load i64, i64* %var_pkt_1379
  %t64_1442 = load i64, i64* %param_payload_len_1375
  %t_1443 = add i64 42, %t64_1442
  %fn_ret_1444 = call i64 @__omni_fn_cyw_tx_eth(i64 %t64_1440, i64 %t64_1441, i64 %t_1443, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  ret i64 %fn_ret_1444
}


define void @__omni_trampoline_send_udp_pkt(i8* %raw_arg) {
entry:
  %raw_int = ptrtoint i8* %raw_arg to i64
  %res = call i64 @__omni_cyw_priv_send_udp_pkt(i64 %raw_int, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  ret void
}

define void @__omni_trampoline_pack_send_udp_pkt(i8* %raw_pack) {
entry:
  %p64 = bitcast i8* %raw_pack to i64*
  %p0 = getelementptr inbounds i64, i64* %p64, i64 0
  %a0 = load i64, i64* %p0
  %p1 = getelementptr inbounds i64, i64* %p64, i64 1
  %a1 = load i64, i64* %p1
  %p2 = getelementptr inbounds i64, i64* %p64, i64 2
  %a2 = load i64, i64* %p2
  %p3 = getelementptr inbounds i64, i64* %p64, i64 3
  %a3 = load i64, i64* %p3
  %p4 = getelementptr inbounds i64, i64* %p64, i64 4
  %a4 = load i64, i64* %p4
  %p5 = getelementptr inbounds i64, i64* %p64, i64 5
  %a5 = load i64, i64* %p5
  call void @free(i8* %raw_pack)
  %res = call i64 @__omni_cyw_priv_send_udp_pkt(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5, i64 -581005069656919554, i64 -581005069656919554)
  ret void
}

define i64 @__omni_cyw_priv_send_discovery_beacon(i64 %arg0, i64 %arg1, i64 %arg2, i64 %arg3, i64 %arg4, i64 %arg5) {
entry:
  %param_ctx_1446 = alloca i64
  %param_dmac_1448 = alloca i64
  %param_dip_1450 = alloca i64
  %param_dport_1452 = alloca i64
  %var_my_ip_1455 = alloca i64
  %var_pay_1463 = alloca i64
  %var_pos_1464 = alloca i64
  %var_ip_s_1475 = alloca i64
  %var_il_1478 = alloca i64
  %param_unset_ctx_1445 = icmp eq i64 %arg0, -581005069656919554
  %param_dsel_ctx_1445 = select i1 %param_unset_ctx_1445, i64 0, i64 %arg0
  store i64 %param_dsel_ctx_1445, i64* %param_ctx_1446
  %param_unset_dmac_1447 = icmp eq i64 %arg1, -581005069656919554
  %param_dsel_dmac_1447 = select i1 %param_unset_dmac_1447, i64 0, i64 %arg1
  store i64 %param_dsel_dmac_1447, i64* %param_dmac_1448
  %param_unset_dip_1449 = icmp eq i64 %arg2, -581005069656919554
  %param_dsel_dip_1449 = select i1 %param_unset_dip_1449, i64 0, i64 %arg2
  store i64 %param_dsel_dip_1449, i64* %param_dip_1450
  %param_unset_dport_1451 = icmp eq i64 %arg3, -581005069656919554
  %param_dsel_dport_1451 = select i1 %param_unset_dport_1451, i64 0, i64 %arg3
  store i64 %param_dsel_dport_1451, i64* %param_dport_1452
  call void @__omni_c_return_ptr(i8* null)
  %t64_1453 = load i64, i64* %param_ctx_1446
  %t_1454 = add i64 %t64_1453, 40
  store i64 %t_1454, i64* %var_my_ip_1455
  %t64_1456 = load i64, i64* %var_my_ip_1455
  %fn_ret_1457 = call i64 @__omni_cyw_priv_ip_is_zero(i64 %t64_1456, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %cond_1458 = icmp ne i64 %fn_ret_1457, 0
  br i1 %cond_1458, label %if_then_42, label %if_merge_42

if_then_42:
  ret i64 0
if_merge_42:
  call void @__omni_c_return_ptr(i8* null)
  %t64_1459 = load i64, i64* %param_ctx_1446
  %t_1460 = add i64 %t64_1459, 4672
  %t_1461 = add i64 %t_1460, 18
  %t_1462 = add i64 %t_1461, 42
  store i64 %t_1462, i64* %var_pay_1463
  call void @__omni_c_return_ptr(i8* null)
  store i64 0, i64* %var_pos_1464
  call void @__omni_c_return_ptr(i8* null)
  %t64_1465 = load i64, i64* %var_pos_1464
  %t64_1466 = load i64, i64* %var_pay_1463
  %t64_1467 = load i64, i64* %var_pos_1464
  %t_1468 = add i64 %t64_1466, %t64_1467
  %str_lit_1469 = getelementptr inbounds [16 x i8], [16 x i8]* @.str.cyw_5, i64 0, i64 0
  call void @__omni_c_return_ptr(i8* %str_lit_1469)
  %p2i64_1470 = ptrtoint i8* %str_lit_1469 to i64
  %fn_ret_1471 = call i64 @__omni_cyw_priv_put_ascii(i64 %t_1468, i64 %p2i64_1470, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t_1472 = add i64 %t64_1465, %fn_ret_1471
  store i64 %t_1472, i64* %var_pos_1464
  call void @__omni_c_return_ptr(i8* null)
  %t64_1473 = load i64, i64* %param_ctx_1446
  %t_1474 = add i64 %t64_1473, 128
  store i64 %t_1474, i64* %var_ip_s_1475
  call void @__omni_c_return_ptr(i8* null)
  %t64_1476 = load i64, i64* %var_ip_s_1475
  %fn_ret_1477 = call i64 @__omni_cyw_priv_cstr_len(i64 %t64_1476, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  store i64 %fn_ret_1477, i64* %var_il_1478
  %t64_1479 = load i64, i64* %var_pay_1463
  %t64_1480 = load i64, i64* %var_pos_1464
  %t_1481 = add i64 %t64_1479, %t64_1480
  %t64_1482 = load i64, i64* %var_ip_s_1475
  %t64_1483 = load i64, i64* %var_il_1478
  %fn_ret_1484 = call i64 @__omni_cyw_priv_bcopy(i64 %t_1481, i64 %t64_1482, i64 %t64_1483, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  call void @__omni_c_return_ptr(i8* null)
  %t64_1485 = load i64, i64* %var_pos_1464
  %t64_1486 = load i64, i64* %var_il_1478
  %t_1487 = add i64 %t64_1485, %t64_1486
  store i64 %t_1487, i64* %var_pos_1464
  call void @__omni_c_return_ptr(i8* null)
  %t64_1488 = load i64, i64* %var_pos_1464
  %t64_1489 = load i64, i64* %var_pay_1463
  %t64_1490 = load i64, i64* %var_pos_1464
  %t_1491 = add i64 %t64_1489, %t64_1490
  %str_lit_1492 = getelementptr inbounds [10 x i8], [10 x i8]* @.str.cyw_6, i64 0, i64 0
  call void @__omni_c_return_ptr(i8* %str_lit_1492)
  %p2i64_1493 = ptrtoint i8* %str_lit_1492 to i64
  %fn_ret_1494 = call i64 @__omni_cyw_priv_put_ascii(i64 %t_1491, i64 %p2i64_1493, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t_1495 = add i64 %t64_1488, %fn_ret_1494
  store i64 %t_1495, i64* %var_pos_1464
  call void @__omni_c_return_ptr(i8* null)
  %t64_1496 = load i64, i64* %var_pos_1464
  %t64_1497 = load i64, i64* %var_pay_1463
  %t64_1498 = load i64, i64* %var_pos_1464
  %t_1499 = add i64 %t64_1497, %t64_1498
  %t64_1500 = load i64, i64* %param_ctx_1446
  %t_1501 = add i64 %t64_1500, 16
  %ext_i64_1502 = call i64 @mmio_read32(i64 %t_1501)
  %fn_ret_1503 = call i64 @__omni_cyw_priv_write_u16_dec(i64 %t_1499, i64 %ext_i64_1502, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t_1504 = add i64 %t64_1496, %fn_ret_1503
  store i64 %t_1504, i64* %var_pos_1464
  %t64_1505 = load i64, i64* %var_pay_1463
  %t64_1506 = load i64, i64* %var_pos_1464
  %t_1507 = add i64 %t64_1505, %t64_1506
  %fn_ret_1508 = call i64 @__omni_cyw_priv_w8(i64 %t_1507, i64 10, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  call void @__omni_c_return_ptr(i8* null)
  %t64_1509 = load i64, i64* %var_pos_1464
  %t_1510 = add i64 %t64_1509, 1
  store i64 %t_1510, i64* %var_pos_1464
  %t64_1511 = load i64, i64* %param_ctx_1446
  %t64_1512 = load i64, i64* %param_dmac_1448
  %t64_1513 = load i64, i64* %var_my_ip_1455
  %t64_1514 = load i64, i64* %param_dip_1450
  %t64_1515 = load i64, i64* %param_dport_1452
  %t64_1516 = load i64, i64* %var_pos_1464
  %fn_ret_1517 = call i64 @__omni_cyw_priv_send_udp_pkt(i64 %t64_1511, i64 %t64_1512, i64 %t64_1513, i64 %t64_1514, i64 1425, i64 %t64_1515, i64 0, i64 %t64_1516)
  ret i64 %fn_ret_1517
}


define void @__omni_trampoline_send_discovery_beacon(i8* %raw_arg) {
entry:
  %raw_int = ptrtoint i8* %raw_arg to i64
  %res = call i64 @__omni_cyw_priv_send_discovery_beacon(i64 %raw_int, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  ret void
}

define void @__omni_trampoline_pack_send_discovery_beacon(i8* %raw_pack) {
entry:
  %p64 = bitcast i8* %raw_pack to i64*
  %p0 = getelementptr inbounds i64, i64* %p64, i64 0
  %a0 = load i64, i64* %p0
  %p1 = getelementptr inbounds i64, i64* %p64, i64 1
  %a1 = load i64, i64* %p1
  %p2 = getelementptr inbounds i64, i64* %p64, i64 2
  %a2 = load i64, i64* %p2
  %p3 = getelementptr inbounds i64, i64* %p64, i64 3
  %a3 = load i64, i64* %p3
  %p4 = getelementptr inbounds i64, i64* %p64, i64 4
  %a4 = load i64, i64* %p4
  %p5 = getelementptr inbounds i64, i64* %p64, i64 5
  %a5 = load i64, i64* %p5
  call void @free(i8* %raw_pack)
  %res = call i64 @__omni_cyw_priv_send_discovery_beacon(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5)
  ret void
}

define i64 @__omni_cyw_priv_broadcast_discovery(i64 %arg0, i64 %arg1, i64 %arg2, i64 %arg3, i64 %arg4, i64 %arg5) {
entry:
  %param_ctx_1519 = alloca i64
  %var_scratch_1522 = alloca i64
  %var_bmac_1524 = alloca i64
  %var_bip_1527 = alloca i64
  %var_sub_bip_1530 = alloca i64
  %var_i_1531 = alloca i64
  %var_ipb_1551 = alloca i64
  %var_msk_1557 = alloca i64
  %param_unset_ctx_1518 = icmp eq i64 %arg0, -581005069656919554
  %param_dsel_ctx_1518 = select i1 %param_unset_ctx_1518, i64 0, i64 %arg0
  store i64 %param_dsel_ctx_1518, i64* %param_ctx_1519
  call void @__omni_c_return_ptr(i8* null)
  %t64_1520 = load i64, i64* %param_ctx_1519
  %t_1521 = add i64 %t64_1520, 8000
  store i64 %t_1521, i64* %var_scratch_1522
  call void @__omni_c_return_ptr(i8* null)
  %t64_1523 = load i64, i64* %var_scratch_1522
  store i64 %t64_1523, i64* %var_bmac_1524
  call void @__omni_c_return_ptr(i8* null)
  %t64_1525 = load i64, i64* %var_scratch_1522
  %t_1526 = add i64 %t64_1525, 8
  store i64 %t_1526, i64* %var_bip_1527
  call void @__omni_c_return_ptr(i8* null)
  %t64_1528 = load i64, i64* %var_scratch_1522
  %t_1529 = add i64 %t64_1528, 16
  store i64 %t_1529, i64* %var_sub_bip_1530
  call void @__omni_c_return_ptr(i8* null)
  store i64 0, i64* %var_i_1531
  br label %while_cond_43

while_cond_43:
  %t64_1532 = load i64, i64* %var_i_1531
  %cond_1533 = icmp slt i64 %t64_1532, 6
  br i1 %cond_1533, label %while_body_43, label %while_exit_43

while_body_43:
  %t64_1534 = load i64, i64* %var_bmac_1524
  %t64_1535 = load i64, i64* %var_i_1531
  %t_1536 = add i64 %t64_1534, %t64_1535
  %fn_ret_1537 = call i64 @__omni_cyw_priv_w8(i64 %t_1536, i64 255, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  call void @__omni_c_return_ptr(i8* null)
  %t64_1538 = load i64, i64* %var_i_1531
  %t_1539 = add i64 %t64_1538, 1
  store i64 %t_1539, i64* %var_i_1531
  br label %while_cond_43

while_exit_43:
  call void @__omni_c_return_ptr(i8* null)
  store i64 0, i64* %var_i_1531
  br label %while_cond_44

while_cond_44:
  %t64_1540 = load i64, i64* %var_i_1531
  %cond_1541 = icmp slt i64 %t64_1540, 4
  br i1 %cond_1541, label %while_body_44, label %while_exit_44

while_body_44:
  %t64_1542 = load i64, i64* %var_bip_1527
  %t64_1543 = load i64, i64* %var_i_1531
  %t_1544 = add i64 %t64_1542, %t64_1543
  %fn_ret_1545 = call i64 @__omni_cyw_priv_w8(i64 %t_1544, i64 255, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  call void @__omni_c_return_ptr(i8* null)
  %t64_1546 = load i64, i64* %param_ctx_1519
  %t_1547 = add i64 %t64_1546, 40
  %t64_1548 = load i64, i64* %var_i_1531
  %t_1549 = add i64 %t_1547, %t64_1548
  %fn_ret_1550 = call i64 @__omni_cyw_priv_r8(i64 %t_1549, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  store i64 %fn_ret_1550, i64* %var_ipb_1551
  call void @__omni_c_return_ptr(i8* null)
  %t64_1552 = load i64, i64* %param_ctx_1519
  %t_1553 = add i64 %t64_1552, 44
  %t64_1554 = load i64, i64* %var_i_1531
  %t_1555 = add i64 %t_1553, %t64_1554
  %fn_ret_1556 = call i64 @__omni_cyw_priv_r8(i64 %t_1555, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  store i64 %fn_ret_1556, i64* %var_msk_1557
  %t64_1558 = load i64, i64* %var_msk_1557
  %cond_1559 = icmp eq i64 %t64_1558, 0
  br i1 %cond_1559, label %if_then_45, label %if_merge_45

if_then_45:
  call void @__omni_c_return_ptr(i8* null)
  store i64 255, i64* %var_msk_1557
  %t64_1560 = load i64, i64* %var_i_1531
  %cond_1561 = icmp eq i64 %t64_1560, 3
  br i1 %cond_1561, label %if_then_46, label %if_merge_46

if_then_46:
  call void @__omni_c_return_ptr(i8* null)
  store i64 0, i64* %var_msk_1557
  br label %if_merge_46

if_merge_46:
  br label %if_merge_45

if_merge_45:
  %t64_1562 = load i64, i64* %var_sub_bip_1530
  %t64_1563 = load i64, i64* %var_i_1531
  %t_1564 = add i64 %t64_1562, %t64_1563
  %t64_1565 = load i64, i64* %var_ipb_1551
  %t64_1566 = load i64, i64* %var_msk_1557
  %t_1567 = xor i64 %t64_1566, 255
  %t_1568 = or i64 %t64_1565, %t_1567
  %t_1569 = and i64 %t_1568, 255
  %fn_ret_1570 = call i64 @__omni_cyw_priv_w8(i64 %t_1564, i64 %t_1569, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  call void @__omni_c_return_ptr(i8* null)
  %t64_1571 = load i64, i64* %var_i_1531
  %t_1572 = add i64 %t64_1571, 1
  store i64 %t_1572, i64* %var_i_1531
  br label %while_cond_44

while_exit_44:
  %t64_1573 = load i64, i64* %param_ctx_1519
  %t64_1574 = load i64, i64* %var_bmac_1524
  %t64_1575 = load i64, i64* %var_sub_bip_1530
  %fn_ret_1576 = call i64 @__omni_cyw_priv_send_discovery_beacon(i64 %t64_1573, i64 %t64_1574, i64 %t64_1575, i64 1425, i64 -581005069656919554, i64 -581005069656919554)
  %t64_1577 = load i64, i64* %param_ctx_1519
  %t64_1578 = load i64, i64* %var_bmac_1524
  %t64_1579 = load i64, i64* %var_bip_1527
  %fn_ret_1580 = call i64 @__omni_cyw_priv_send_discovery_beacon(i64 %t64_1577, i64 %t64_1578, i64 %t64_1579, i64 1425, i64 -581005069656919554, i64 -581005069656919554)
  ret i64 0
}


define void @__omni_trampoline_broadcast_discovery(i8* %raw_arg) {
entry:
  %raw_int = ptrtoint i8* %raw_arg to i64
  %res = call i64 @__omni_cyw_priv_broadcast_discovery(i64 %raw_int, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  ret void
}

define void @__omni_trampoline_pack_broadcast_discovery(i8* %raw_pack) {
entry:
  %p64 = bitcast i8* %raw_pack to i64*
  %p0 = getelementptr inbounds i64, i64* %p64, i64 0
  %a0 = load i64, i64* %p0
  %p1 = getelementptr inbounds i64, i64* %p64, i64 1
  %a1 = load i64, i64* %p1
  %p2 = getelementptr inbounds i64, i64* %p64, i64 2
  %a2 = load i64, i64* %p2
  %p3 = getelementptr inbounds i64, i64* %p64, i64 3
  %a3 = load i64, i64* %p3
  %p4 = getelementptr inbounds i64, i64* %p64, i64 4
  %a4 = load i64, i64* %p4
  %p5 = getelementptr inbounds i64, i64* %p64, i64 5
  %a5 = load i64, i64* %p5
  call void @free(i8* %raw_pack)
  %res = call i64 @__omni_cyw_priv_broadcast_discovery(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5)
  ret void
}

define i64 @__omni_cyw_priv_dhcp_send(i64 %arg0, i64 %arg1, i64 %arg2, i64 %arg3, i64 %arg4, i64 %arg5) {
entry:
  %param_ctx_1582 = alloca i64
  %param_msg_type_1584 = alloca i64
  %var_pay_1589 = alloca i64
  %var_op_1627 = alloca i64
  %var_hl_1703 = alloca i64
  %var_scratch_1751 = alloca i64
  %var_bmac_1753 = alloca i64
  %var_zip_1756 = alloca i64
  %var_bip_1759 = alloca i64
  %var_i_1760 = alloca i64
  %param_unset_ctx_1581 = icmp eq i64 %arg0, -581005069656919554
  %param_dsel_ctx_1581 = select i1 %param_unset_ctx_1581, i64 0, i64 %arg0
  store i64 %param_dsel_ctx_1581, i64* %param_ctx_1582
  %param_unset_msg_type_1583 = icmp eq i64 %arg1, -581005069656919554
  %param_dsel_msg_type_1583 = select i1 %param_unset_msg_type_1583, i64 0, i64 %arg1
  store i64 %param_dsel_msg_type_1583, i64* %param_msg_type_1584
  call void @__omni_c_return_ptr(i8* null)
  %t64_1585 = load i64, i64* %param_ctx_1582
  %t_1586 = add i64 %t64_1585, 4672
  %t_1587 = add i64 %t_1586, 18
  %t_1588 = add i64 %t_1587, 42
  store i64 %t_1588, i64* %var_pay_1589
  %t64_1590 = load i64, i64* %var_pay_1589
  %fn_ret_1591 = call i64 @__omni_cyw_priv_bzero(i64 %t64_1590, i64 284, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_1592 = load i64, i64* %var_pay_1589
  %t_1593 = add i64 %t64_1592, 0
  %fn_ret_1594 = call i64 @__omni_cyw_priv_w8(i64 %t_1593, i64 1, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_1595 = load i64, i64* %var_pay_1589
  %t_1596 = add i64 %t64_1595, 1
  %fn_ret_1597 = call i64 @__omni_cyw_priv_w8(i64 %t_1596, i64 1, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_1598 = load i64, i64* %var_pay_1589
  %t_1599 = add i64 %t64_1598, 2
  %fn_ret_1600 = call i64 @__omni_cyw_priv_w8(i64 %t_1599, i64 6, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_1601 = load i64, i64* %var_pay_1589
  %t_1602 = add i64 %t64_1601, 4
  %t64_1603 = load i64, i64* %param_ctx_1582
  %t_1604 = add i64 %t64_1603, 24
  %ext_i64_1605 = call i64 @mmio_read32(i64 %t_1604)
  %fn_ret_1606 = call i64 @__omni_cyw_priv_w32_be(i64 %t_1602, i64 %ext_i64_1605, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_1607 = load i64, i64* %var_pay_1589
  %t_1608 = add i64 %t64_1607, 10
  %fn_ret_1609 = call i64 @__omni_cyw_priv_w16_be(i64 %t_1608, i64 32768, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_1610 = load i64, i64* %var_pay_1589
  %t_1611 = add i64 %t64_1610, 28
  %t64_1612 = load i64, i64* %param_ctx_1582
  %t_1613 = add i64 %t64_1612, 32
  %fn_ret_1614 = call i64 @__omni_cyw_priv_bcopy(i64 %t_1611, i64 %t_1613, i64 6, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_1615 = load i64, i64* %var_pay_1589
  %t_1616 = add i64 %t64_1615, 236
  %fn_ret_1617 = call i64 @__omni_cyw_priv_w8(i64 %t_1616, i64 99, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_1618 = load i64, i64* %var_pay_1589
  %t_1619 = add i64 %t64_1618, 237
  %fn_ret_1620 = call i64 @__omni_cyw_priv_w8(i64 %t_1619, i64 130, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_1621 = load i64, i64* %var_pay_1589
  %t_1622 = add i64 %t64_1621, 238
  %fn_ret_1623 = call i64 @__omni_cyw_priv_w8(i64 %t_1622, i64 83, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_1624 = load i64, i64* %var_pay_1589
  %t_1625 = add i64 %t64_1624, 239
  %fn_ret_1626 = call i64 @__omni_cyw_priv_w8(i64 %t_1625, i64 99, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  call void @__omni_c_return_ptr(i8* null)
  store i64 240, i64* %var_op_1627
  %t64_1628 = load i64, i64* %var_pay_1589
  %t64_1629 = load i64, i64* %var_op_1627
  %t_1630 = add i64 %t64_1628, %t64_1629
  %fn_ret_1631 = call i64 @__omni_cyw_priv_w8(i64 %t_1630, i64 53, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_1632 = load i64, i64* %var_pay_1589
  %t64_1633 = load i64, i64* %var_op_1627
  %t_1634 = add i64 %t64_1632, %t64_1633
  %t_1635 = add i64 %t_1634, 1
  %fn_ret_1636 = call i64 @__omni_cyw_priv_w8(i64 %t_1635, i64 1, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_1637 = load i64, i64* %var_pay_1589
  %t64_1638 = load i64, i64* %var_op_1627
  %t_1639 = add i64 %t64_1637, %t64_1638
  %t_1640 = add i64 %t_1639, 2
  %t64_1641 = load i64, i64* %param_msg_type_1584
  %fn_ret_1642 = call i64 @__omni_cyw_priv_w8(i64 %t_1640, i64 %t64_1641, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  call void @__omni_c_return_ptr(i8* null)
  %t64_1643 = load i64, i64* %var_op_1627
  %t_1644 = add i64 %t64_1643, 3
  store i64 %t_1644, i64* %var_op_1627
  %t64_1645 = load i64, i64* %param_msg_type_1584
  %cond_1646 = icmp eq i64 %t64_1645, 3
  br i1 %cond_1646, label %if_then_47, label %if_merge_47

if_then_47:
  %t64_1647 = load i64, i64* %var_pay_1589
  %t64_1648 = load i64, i64* %var_op_1627
  %t_1649 = add i64 %t64_1647, %t64_1648
  %fn_ret_1650 = call i64 @__omni_cyw_priv_w8(i64 %t_1649, i64 50, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_1651 = load i64, i64* %var_pay_1589
  %t64_1652 = load i64, i64* %var_op_1627
  %t_1653 = add i64 %t64_1651, %t64_1652
  %t_1654 = add i64 %t_1653, 1
  %fn_ret_1655 = call i64 @__omni_cyw_priv_w8(i64 %t_1654, i64 4, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_1656 = load i64, i64* %var_pay_1589
  %t64_1657 = load i64, i64* %var_op_1627
  %t_1658 = add i64 %t64_1656, %t64_1657
  %t_1659 = add i64 %t_1658, 2
  %t64_1660 = load i64, i64* %param_ctx_1582
  %t_1661 = add i64 %t64_1660, 52
  %fn_ret_1662 = call i64 @__omni_cyw_priv_bcopy(i64 %t_1659, i64 %t_1661, i64 4, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  call void @__omni_c_return_ptr(i8* null)
  %t64_1663 = load i64, i64* %var_op_1627
  %t_1664 = add i64 %t64_1663, 6
  store i64 %t_1664, i64* %var_op_1627
  %t64_1665 = load i64, i64* %param_ctx_1582
  %t_1666 = add i64 %t64_1665, 60
  %ext_i64_1667 = call i64 @mmio_read32(i64 %t_1666)
  %cond_1668 = icmp ne i64 %ext_i64_1667, 0
  br i1 %cond_1668, label %if_then_48, label %if_merge_48

if_then_48:
  %t64_1669 = load i64, i64* %var_pay_1589
  %t64_1670 = load i64, i64* %var_op_1627
  %t_1671 = add i64 %t64_1669, %t64_1670
  %fn_ret_1672 = call i64 @__omni_cyw_priv_w8(i64 %t_1671, i64 54, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_1673 = load i64, i64* %var_pay_1589
  %t64_1674 = load i64, i64* %var_op_1627
  %t_1675 = add i64 %t64_1673, %t64_1674
  %t_1676 = add i64 %t_1675, 1
  %fn_ret_1677 = call i64 @__omni_cyw_priv_w8(i64 %t_1676, i64 4, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_1678 = load i64, i64* %var_pay_1589
  %t64_1679 = load i64, i64* %var_op_1627
  %t_1680 = add i64 %t64_1678, %t64_1679
  %t_1681 = add i64 %t_1680, 2
  %t64_1682 = load i64, i64* %param_ctx_1582
  %t_1683 = add i64 %t64_1682, 56
  %fn_ret_1684 = call i64 @__omni_cyw_priv_bcopy(i64 %t_1681, i64 %t_1683, i64 4, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  call void @__omni_c_return_ptr(i8* null)
  %t64_1685 = load i64, i64* %var_op_1627
  %t_1686 = add i64 %t64_1685, 6
  store i64 %t_1686, i64* %var_op_1627
  br label %if_merge_48

if_merge_48:
  br label %if_merge_47

if_merge_47:
  %t64_1687 = load i64, i64* %var_pay_1589
  %t64_1688 = load i64, i64* %var_op_1627
  %t_1689 = add i64 %t64_1687, %t64_1688
  %fn_ret_1690 = call i64 @__omni_cyw_priv_w8(i64 %t_1689, i64 12, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_1691 = load i64, i64* %var_pay_1589
  %t64_1692 = load i64, i64* %var_op_1627
  %t_1693 = add i64 %t64_1691, %t64_1692
  %t_1694 = add i64 %t_1693, 1
  %fn_ret_1695 = call i64 @__omni_cyw_priv_w8(i64 %t_1694, i64 11, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  call void @__omni_c_return_ptr(i8* null)
  %t64_1696 = load i64, i64* %var_pay_1589
  %t64_1697 = load i64, i64* %var_op_1627
  %t_1698 = add i64 %t64_1696, %t64_1697
  %t_1699 = add i64 %t_1698, 2
  %str_lit_1700 = getelementptr inbounds [12 x i8], [12 x i8]* @.str.cyw_7, i64 0, i64 0
  call void @__omni_c_return_ptr(i8* %str_lit_1700)
  %p2i64_1701 = ptrtoint i8* %str_lit_1700 to i64
  %fn_ret_1702 = call i64 @__omni_cyw_priv_put_ascii(i64 %t_1699, i64 %p2i64_1701, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  store i64 %fn_ret_1702, i64* %var_hl_1703
  call void @__omni_c_return_ptr(i8* null)
  %t64_1704 = load i64, i64* %var_op_1627
  %t_1705 = add i64 %t64_1704, 2
  %t64_1706 = load i64, i64* %var_hl_1703
  %t_1707 = add i64 %t_1705, %t64_1706
  store i64 %t_1707, i64* %var_op_1627
  %t64_1708 = load i64, i64* %var_pay_1589
  %t64_1709 = load i64, i64* %var_op_1627
  %t_1710 = add i64 %t64_1708, %t64_1709
  %fn_ret_1711 = call i64 @__omni_cyw_priv_w8(i64 %t_1710, i64 55, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_1712 = load i64, i64* %var_pay_1589
  %t64_1713 = load i64, i64* %var_op_1627
  %t_1714 = add i64 %t64_1712, %t64_1713
  %t_1715 = add i64 %t_1714, 1
  %fn_ret_1716 = call i64 @__omni_cyw_priv_w8(i64 %t_1715, i64 3, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_1717 = load i64, i64* %var_pay_1589
  %t64_1718 = load i64, i64* %var_op_1627
  %t_1719 = add i64 %t64_1717, %t64_1718
  %t_1720 = add i64 %t_1719, 2
  %fn_ret_1721 = call i64 @__omni_cyw_priv_w8(i64 %t_1720, i64 1, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_1722 = load i64, i64* %var_pay_1589
  %t64_1723 = load i64, i64* %var_op_1627
  %t_1724 = add i64 %t64_1722, %t64_1723
  %t_1725 = add i64 %t_1724, 3
  %fn_ret_1726 = call i64 @__omni_cyw_priv_w8(i64 %t_1725, i64 3, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_1727 = load i64, i64* %var_pay_1589
  %t64_1728 = load i64, i64* %var_op_1627
  %t_1729 = add i64 %t64_1727, %t64_1728
  %t_1730 = add i64 %t_1729, 4
  %fn_ret_1731 = call i64 @__omni_cyw_priv_w8(i64 %t_1730, i64 6, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  call void @__omni_c_return_ptr(i8* null)
  %t64_1732 = load i64, i64* %var_op_1627
  %t_1733 = add i64 %t64_1732, 5
  store i64 %t_1733, i64* %var_op_1627
  %t64_1734 = load i64, i64* %var_pay_1589
  %t64_1735 = load i64, i64* %var_op_1627
  %t_1736 = add i64 %t64_1734, %t64_1735
  %fn_ret_1737 = call i64 @__omni_cyw_priv_w8(i64 %t_1736, i64 255, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  call void @__omni_c_return_ptr(i8* null)
  %t64_1738 = load i64, i64* %var_op_1627
  %t_1739 = add i64 %t64_1738, 1
  store i64 %t_1739, i64* %var_op_1627
  br label %while_cond_49

while_cond_49:
  %t64_1740 = load i64, i64* %var_op_1627
  %t_1741 = and i64 %t64_1740, 3
  %cond_1742 = icmp ne i64 %t_1741, 0
  br i1 %cond_1742, label %while_body_49, label %while_exit_49

while_body_49:
  %t64_1743 = load i64, i64* %var_pay_1589
  %t64_1744 = load i64, i64* %var_op_1627
  %t_1745 = add i64 %t64_1743, %t64_1744
  %fn_ret_1746 = call i64 @__omni_cyw_priv_w8(i64 %t_1745, i64 0, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  call void @__omni_c_return_ptr(i8* null)
  %t64_1747 = load i64, i64* %var_op_1627
  %t_1748 = add i64 %t64_1747, 1
  store i64 %t_1748, i64* %var_op_1627
  br label %while_cond_49

while_exit_49:
  call void @__omni_c_return_ptr(i8* null)
  %t64_1749 = load i64, i64* %param_ctx_1582
  %t_1750 = add i64 %t64_1749, 8000
  store i64 %t_1750, i64* %var_scratch_1751
  call void @__omni_c_return_ptr(i8* null)
  %t64_1752 = load i64, i64* %var_scratch_1751
  store i64 %t64_1752, i64* %var_bmac_1753
  call void @__omni_c_return_ptr(i8* null)
  %t64_1754 = load i64, i64* %var_scratch_1751
  %t_1755 = add i64 %t64_1754, 8
  store i64 %t_1755, i64* %var_zip_1756
  call void @__omni_c_return_ptr(i8* null)
  %t64_1757 = load i64, i64* %var_scratch_1751
  %t_1758 = add i64 %t64_1757, 16
  store i64 %t_1758, i64* %var_bip_1759
  call void @__omni_c_return_ptr(i8* null)
  store i64 0, i64* %var_i_1760
  br label %while_cond_50

while_cond_50:
  %t64_1761 = load i64, i64* %var_i_1760
  %cond_1762 = icmp slt i64 %t64_1761, 6
  br i1 %cond_1762, label %while_body_50, label %while_exit_50

while_body_50:
  %t64_1763 = load i64, i64* %var_bmac_1753
  %t64_1764 = load i64, i64* %var_i_1760
  %t_1765 = add i64 %t64_1763, %t64_1764
  %fn_ret_1766 = call i64 @__omni_cyw_priv_w8(i64 %t_1765, i64 255, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  call void @__omni_c_return_ptr(i8* null)
  %t64_1767 = load i64, i64* %var_i_1760
  %t_1768 = add i64 %t64_1767, 1
  store i64 %t_1768, i64* %var_i_1760
  br label %while_cond_50

while_exit_50:
  %t64_1769 = load i64, i64* %var_zip_1756
  %fn_ret_1770 = call i64 @__omni_cyw_priv_bzero(i64 %t64_1769, i64 4, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  call void @__omni_c_return_ptr(i8* null)
  store i64 0, i64* %var_i_1760
  br label %while_cond_51

while_cond_51:
  %t64_1771 = load i64, i64* %var_i_1760
  %cond_1772 = icmp slt i64 %t64_1771, 4
  br i1 %cond_1772, label %while_body_51, label %while_exit_51

while_body_51:
  %t64_1773 = load i64, i64* %var_bip_1759
  %t64_1774 = load i64, i64* %var_i_1760
  %t_1775 = add i64 %t64_1773, %t64_1774
  %fn_ret_1776 = call i64 @__omni_cyw_priv_w8(i64 %t_1775, i64 255, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  call void @__omni_c_return_ptr(i8* null)
  %t64_1777 = load i64, i64* %var_i_1760
  %t_1778 = add i64 %t64_1777, 1
  store i64 %t_1778, i64* %var_i_1760
  br label %while_cond_51

while_exit_51:
  %t64_1779 = load i64, i64* %param_ctx_1582
  %t64_1780 = load i64, i64* %var_bmac_1753
  %t64_1781 = load i64, i64* %var_zip_1756
  %t64_1782 = load i64, i64* %var_bip_1759
  %t64_1783 = load i64, i64* %var_op_1627
  %fn_ret_1784 = call i64 @__omni_cyw_priv_send_udp_pkt(i64 %t64_1779, i64 %t64_1780, i64 %t64_1781, i64 %t64_1782, i64 68, i64 67, i64 0, i64 %t64_1783)
  ret i64 %fn_ret_1784
}


define void @__omni_trampoline_dhcp_send(i8* %raw_arg) {
entry:
  %raw_int = ptrtoint i8* %raw_arg to i64
  %res = call i64 @__omni_cyw_priv_dhcp_send(i64 %raw_int, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  ret void
}

define void @__omni_trampoline_pack_dhcp_send(i8* %raw_pack) {
entry:
  %p64 = bitcast i8* %raw_pack to i64*
  %p0 = getelementptr inbounds i64, i64* %p64, i64 0
  %a0 = load i64, i64* %p0
  %p1 = getelementptr inbounds i64, i64* %p64, i64 1
  %a1 = load i64, i64* %p1
  %p2 = getelementptr inbounds i64, i64* %p64, i64 2
  %a2 = load i64, i64* %p2
  %p3 = getelementptr inbounds i64, i64* %p64, i64 3
  %a3 = load i64, i64* %p3
  %p4 = getelementptr inbounds i64, i64* %p64, i64 4
  %a4 = load i64, i64* %p4
  %p5 = getelementptr inbounds i64, i64* %p64, i64 5
  %a5 = load i64, i64* %p5
  call void @free(i8* %raw_pack)
  %res = call i64 @__omni_cyw_priv_dhcp_send(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5)
  ret void
}

define i64 @__omni_cyw_priv_dhcp_rx_handle(i64 %arg0, i64 %arg1, i64 %arg2, i64 %arg3, i64 %arg4, i64 %arg5) {
entry:
  %param_ctx_1786 = alloca i64
  %param_eth_1788 = alloca i64
  %param_dhcp_1790 = alloca i64
  %param_dhcp_len_1792 = alloca i64
  %var_msg_type_1833 = alloca i64
  %var_has_srv_1834 = alloca i64
  %var_srv_tmp_1838 = alloca i64
  %var_sub_tmp_1842 = alloca i64
  %var_rtr_tmp_1846 = alloca i64
  %var_pos_1849 = alloca i64
  %var_opt_1857 = alloca i64
  %var_olen_1873 = alloca i64
  %var_odata_1878 = alloca i64
  %var_st_1927 = alloca i64
  %param_unset_ctx_1785 = icmp eq i64 %arg0, -581005069656919554
  %param_dsel_ctx_1785 = select i1 %param_unset_ctx_1785, i64 0, i64 %arg0
  store i64 %param_dsel_ctx_1785, i64* %param_ctx_1786
  %param_unset_eth_1787 = icmp eq i64 %arg1, -581005069656919554
  %param_dsel_eth_1787 = select i1 %param_unset_eth_1787, i64 0, i64 %arg1
  store i64 %param_dsel_eth_1787, i64* %param_eth_1788
  %param_unset_dhcp_1789 = icmp eq i64 %arg2, -581005069656919554
  %param_dsel_dhcp_1789 = select i1 %param_unset_dhcp_1789, i64 0, i64 %arg2
  store i64 %param_dsel_dhcp_1789, i64* %param_dhcp_1790
  %param_unset_dhcp_len_1791 = icmp eq i64 %arg3, -581005069656919554
  %param_dsel_dhcp_len_1791 = select i1 %param_unset_dhcp_len_1791, i64 0, i64 %arg3
  store i64 %param_dsel_dhcp_len_1791, i64* %param_dhcp_len_1792
  %t64_1793 = load i64, i64* %param_dhcp_len_1792
  %cond_1794 = icmp slt i64 %t64_1793, 240
  br i1 %cond_1794, label %if_then_52, label %if_merge_52

if_then_52:
  ret i64 0
if_merge_52:
  %t64_1795 = load i64, i64* %param_dhcp_1790
  %fn_ret_1796 = call i64 @__omni_cyw_priv_r8(i64 %t64_1795, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %cond_1797 = icmp ne i64 %fn_ret_1796, 2
  br i1 %cond_1797, label %if_then_53, label %if_merge_53

if_then_53:
  ret i64 0
if_merge_53:
  %t64_1798 = load i64, i64* %param_dhcp_1790
  %t_1799 = add i64 %t64_1798, 4
  %fn_ret_1800 = call i64 @__omni_cyw_priv_r32_be(i64 %t_1799, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_1801 = load i64, i64* %param_ctx_1786
  %t_1802 = add i64 %t64_1801, 24
  %ext_i64_1803 = call i64 @mmio_read32(i64 %t_1802)
  %cond_1804 = icmp ne i64 %fn_ret_1800, %ext_i64_1803
  br i1 %cond_1804, label %if_then_54, label %if_merge_54

if_then_54:
  ret i64 0
if_merge_54:
  %t64_1805 = load i64, i64* %param_dhcp_1790
  %t_1806 = add i64 %t64_1805, 28
  %t64_1807 = load i64, i64* %param_ctx_1786
  %t_1808 = add i64 %t64_1807, 32
  %fn_ret_1809 = call i64 @__omni_cyw_priv_mac_eq(i64 %t_1806, i64 %t_1808, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %cond_1810 = icmp eq i64 %fn_ret_1809, 0
  br i1 %cond_1810, label %if_then_55, label %if_merge_55

if_then_55:
  ret i64 0
if_merge_55:
  %t64_1814 = load i64, i64* %param_dhcp_1790
  %t_1815 = add i64 %t64_1814, 236
  %fn_ret_1816 = call i64 @__omni_cyw_priv_r8(i64 %t_1815, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %cond_1817 = icmp ne i64 %fn_ret_1816, 99
  br i1 %cond_1817, label %sc_or_short_1813, label %sc_or_rhs_1813

sc_or_short_1813:
  br label %sc_or_merge_1813

sc_or_rhs_1813:
  %t64_1818 = load i64, i64* %param_dhcp_1790
  %t_1819 = add i64 %t64_1818, 237
  %fn_ret_1820 = call i64 @__omni_cyw_priv_r8(i64 %t_1819, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %cond_1821 = icmp ne i64 %fn_ret_1820, 130
  br label %sc_or_done_1813

sc_or_done_1813:
  br label %sc_or_merge_1813

sc_or_merge_1813:
  %or_cond_1822 = phi i1 [ true, %sc_or_short_1813 ], [ %cond_1821, %sc_or_done_1813 ]
  br i1 %or_cond_1822, label %sc_or_short_1812, label %sc_or_rhs_1812

sc_or_short_1812:
  br label %sc_or_merge_1812

sc_or_rhs_1812:
  %t64_1823 = load i64, i64* %param_dhcp_1790
  %t_1824 = add i64 %t64_1823, 238
  %fn_ret_1825 = call i64 @__omni_cyw_priv_r8(i64 %t_1824, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %cond_1826 = icmp ne i64 %fn_ret_1825, 83
  br label %sc_or_done_1812

sc_or_done_1812:
  br label %sc_or_merge_1812

sc_or_merge_1812:
  %or_cond_1827 = phi i1 [ true, %sc_or_short_1812 ], [ %cond_1826, %sc_or_done_1812 ]
  br i1 %or_cond_1827, label %sc_or_short_1811, label %sc_or_rhs_1811

sc_or_short_1811:
  br label %sc_or_merge_1811

sc_or_rhs_1811:
  %t64_1828 = load i64, i64* %param_dhcp_1790
  %t_1829 = add i64 %t64_1828, 239
  %fn_ret_1830 = call i64 @__omni_cyw_priv_r8(i64 %t_1829, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %cond_1831 = icmp ne i64 %fn_ret_1830, 99
  br label %sc_or_done_1811

sc_or_done_1811:
  br label %sc_or_merge_1811

sc_or_merge_1811:
  %or_cond_1832 = phi i1 [ true, %sc_or_short_1811 ], [ %cond_1831, %sc_or_done_1811 ]
  br i1 %or_cond_1832, label %if_then_56, label %if_merge_56

if_then_56:
  ret i64 0
if_merge_56:
  call void @__omni_c_return_ptr(i8* null)
  store i64 0, i64* %var_msg_type_1833
  call void @__omni_c_return_ptr(i8* null)
  store i64 0, i64* %var_has_srv_1834
  call void @__omni_c_return_ptr(i8* null)
  %t64_1835 = load i64, i64* %param_ctx_1786
  %t_1836 = add i64 %t64_1835, 8000
  %t_1837 = add i64 %t_1836, 64
  store i64 %t_1837, i64* %var_srv_tmp_1838
  call void @__omni_c_return_ptr(i8* null)
  %t64_1839 = load i64, i64* %param_ctx_1786
  %t_1840 = add i64 %t64_1839, 8000
  %t_1841 = add i64 %t_1840, 68
  store i64 %t_1841, i64* %var_sub_tmp_1842
  call void @__omni_c_return_ptr(i8* null)
  %t64_1843 = load i64, i64* %param_ctx_1786
  %t_1844 = add i64 %t64_1843, 8000
  %t_1845 = add i64 %t_1844, 72
  store i64 %t_1845, i64* %var_rtr_tmp_1846
  %t64_1847 = load i64, i64* %var_sub_tmp_1842
  %fn_ret_1848 = call i64 @__omni_cyw_priv_bzero(i64 %t64_1847, i64 8, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  call void @__omni_c_return_ptr(i8* null)
  store i64 240, i64* %var_pos_1849
  br label %while_cond_57

while_cond_57:
  %t64_1850 = load i64, i64* %var_pos_1849
  %t64_1851 = load i64, i64* %param_dhcp_len_1792
  %cond_1852 = icmp slt i64 %t64_1850, %t64_1851
  br i1 %cond_1852, label %while_body_57, label %while_exit_57

while_body_57:
  call void @__omni_c_return_ptr(i8* null)
  %t64_1853 = load i64, i64* %param_dhcp_1790
  %t64_1854 = load i64, i64* %var_pos_1849
  %t_1855 = add i64 %t64_1853, %t64_1854
  %fn_ret_1856 = call i64 @__omni_cyw_priv_r8(i64 %t_1855, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  store i64 %fn_ret_1856, i64* %var_opt_1857
  %t64_1858 = load i64, i64* %var_opt_1857
  %cond_1859 = icmp eq i64 %t64_1858, 255
  br i1 %cond_1859, label %if_then_58, label %if_merge_58

if_then_58:
  br label %while_exit_57
if_merge_58:
  %t64_1860 = load i64, i64* %var_opt_1857
  %cond_1861 = icmp eq i64 %t64_1860, 0
  br i1 %cond_1861, label %if_then_59, label %if_else_59

if_then_59:
  call void @__omni_c_return_ptr(i8* null)
  %t64_1862 = load i64, i64* %var_pos_1849
  %t_1863 = add i64 %t64_1862, 1
  store i64 %t_1863, i64* %var_pos_1849
  br label %if_merge_59

if_else_59:
  %t64_1864 = load i64, i64* %var_pos_1849
  %t_1865 = add i64 %t64_1864, 1
  %t64_1866 = load i64, i64* %param_dhcp_len_1792
  %cond_1867 = icmp sge i64 %t_1865, %t64_1866
  br i1 %cond_1867, label %if_then_60, label %if_merge_60

if_then_60:
  br label %while_exit_57
if_merge_60:
  call void @__omni_c_return_ptr(i8* null)
  %t64_1868 = load i64, i64* %param_dhcp_1790
  %t64_1869 = load i64, i64* %var_pos_1849
  %t_1870 = add i64 %t64_1868, %t64_1869
  %t_1871 = add i64 %t_1870, 1
  %fn_ret_1872 = call i64 @__omni_cyw_priv_r8(i64 %t_1871, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  store i64 %fn_ret_1872, i64* %var_olen_1873
  call void @__omni_c_return_ptr(i8* null)
  %t64_1874 = load i64, i64* %param_dhcp_1790
  %t64_1875 = load i64, i64* %var_pos_1849
  %t_1876 = add i64 %t64_1874, %t64_1875
  %t_1877 = add i64 %t_1876, 2
  store i64 %t_1877, i64* %var_odata_1878
  %t64_1879 = load i64, i64* %var_pos_1849
  %t_1880 = add i64 %t64_1879, 2
  %t64_1881 = load i64, i64* %var_olen_1873
  %t_1882 = add i64 %t_1880, %t64_1881
  %t64_1883 = load i64, i64* %param_dhcp_len_1792
  %cond_1884 = icmp sgt i64 %t_1882, %t64_1883
  br i1 %cond_1884, label %if_then_61, label %if_merge_61

if_then_61:
  br label %while_exit_57
if_merge_61:
  %t64_1886 = load i64, i64* %var_opt_1857
  %cond_1887 = icmp eq i64 %t64_1886, 53
  br i1 %cond_1887, label %sc_and_rhs_1885, label %sc_and_short_1885

sc_and_short_1885:
  br label %sc_and_merge_1885

sc_and_rhs_1885:
  %t64_1888 = load i64, i64* %var_olen_1873
  %cond_1889 = icmp sge i64 %t64_1888, 1
  br label %sc_and_done_1885

sc_and_done_1885:
  br label %sc_and_merge_1885

sc_and_merge_1885:
  %and_cond_1890 = phi i1 [ false, %sc_and_short_1885 ], [ %cond_1889, %sc_and_done_1885 ]
  br i1 %and_cond_1890, label %if_then_62, label %if_else_62

if_then_62:
  call void @__omni_c_return_ptr(i8* null)
  %t64_1891 = load i64, i64* %var_odata_1878
  %fn_ret_1892 = call i64 @__omni_cyw_priv_r8(i64 %t64_1891, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  store i64 %fn_ret_1892, i64* %var_msg_type_1833
  br label %if_merge_62

if_else_62:
  %t64_1894 = load i64, i64* %var_opt_1857
  %cond_1895 = icmp eq i64 %t64_1894, 54
  br i1 %cond_1895, label %sc_and_rhs_1893, label %sc_and_short_1893

sc_and_short_1893:
  br label %sc_and_merge_1893

sc_and_rhs_1893:
  %t64_1896 = load i64, i64* %var_olen_1873
  %cond_1897 = icmp sge i64 %t64_1896, 4
  br label %sc_and_done_1893

sc_and_done_1893:
  br label %sc_and_merge_1893

sc_and_merge_1893:
  %and_cond_1898 = phi i1 [ false, %sc_and_short_1893 ], [ %cond_1897, %sc_and_done_1893 ]
  br i1 %and_cond_1898, label %if_then_63, label %if_else_63

if_then_63:
  %t64_1899 = load i64, i64* %var_srv_tmp_1838
  %t64_1900 = load i64, i64* %var_odata_1878
  %fn_ret_1901 = call i64 @__omni_cyw_priv_bcopy(i64 %t64_1899, i64 %t64_1900, i64 4, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  call void @__omni_c_return_ptr(i8* null)
  store i64 1, i64* %var_has_srv_1834
  br label %if_merge_63

if_else_63:
  %t64_1903 = load i64, i64* %var_opt_1857
  %cond_1904 = icmp eq i64 %t64_1903, 1
  br i1 %cond_1904, label %sc_and_rhs_1902, label %sc_and_short_1902

sc_and_short_1902:
  br label %sc_and_merge_1902

sc_and_rhs_1902:
  %t64_1905 = load i64, i64* %var_olen_1873
  %cond_1906 = icmp sge i64 %t64_1905, 4
  br label %sc_and_done_1902

sc_and_done_1902:
  br label %sc_and_merge_1902

sc_and_merge_1902:
  %and_cond_1907 = phi i1 [ false, %sc_and_short_1902 ], [ %cond_1906, %sc_and_done_1902 ]
  br i1 %and_cond_1907, label %if_then_64, label %if_else_64

if_then_64:
  %t64_1908 = load i64, i64* %var_sub_tmp_1842
  %t64_1909 = load i64, i64* %var_odata_1878
  %fn_ret_1910 = call i64 @__omni_cyw_priv_bcopy(i64 %t64_1908, i64 %t64_1909, i64 4, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  br label %if_merge_64

if_else_64:
  %t64_1912 = load i64, i64* %var_opt_1857
  %cond_1913 = icmp eq i64 %t64_1912, 3
  br i1 %cond_1913, label %sc_and_rhs_1911, label %sc_and_short_1911

sc_and_short_1911:
  br label %sc_and_merge_1911

sc_and_rhs_1911:
  %t64_1914 = load i64, i64* %var_olen_1873
  %cond_1915 = icmp sge i64 %t64_1914, 4
  br label %sc_and_done_1911

sc_and_done_1911:
  br label %sc_and_merge_1911

sc_and_merge_1911:
  %and_cond_1916 = phi i1 [ false, %sc_and_short_1911 ], [ %cond_1915, %sc_and_done_1911 ]
  br i1 %and_cond_1916, label %if_then_65, label %if_merge_65

if_then_65:
  %t64_1917 = load i64, i64* %var_rtr_tmp_1846
  %t64_1918 = load i64, i64* %var_odata_1878
  %fn_ret_1919 = call i64 @__omni_cyw_priv_bcopy(i64 %t64_1917, i64 %t64_1918, i64 4, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  br label %if_merge_65

if_merge_65:
  br label %if_merge_64

if_merge_64:
  br label %if_merge_63

if_merge_63:
  br label %if_merge_62

if_merge_62:
  call void @__omni_c_return_ptr(i8* null)
  %t64_1920 = load i64, i64* %var_pos_1849
  %t_1921 = add i64 %t64_1920, 2
  %t64_1922 = load i64, i64* %var_olen_1873
  %t_1923 = add i64 %t_1921, %t64_1922
  store i64 %t_1923, i64* %var_pos_1849
  br label %if_merge_59

if_merge_59:
  br label %while_cond_57

while_exit_57:
  call void @__omni_c_return_ptr(i8* null)
  %t64_1924 = load i64, i64* %param_ctx_1786
  %t_1925 = add i64 %t64_1924, 8
  %ext_i64_1926 = call i64 @mmio_read32(i64 %t_1925)
  store i64 %ext_i64_1926, i64* %var_st_1927
  %t64_1929 = load i64, i64* %var_msg_type_1833
  %cond_1930 = icmp eq i64 %t64_1929, 2
  br i1 %cond_1930, label %sc_and_rhs_1928, label %sc_and_short_1928

sc_and_short_1928:
  br label %sc_and_merge_1928

sc_and_rhs_1928:
  %t64_1932 = load i64, i64* %var_st_1927
  %cond_1933 = icmp eq i64 %t64_1932, 1
  br i1 %cond_1933, label %sc_or_short_1931, label %sc_or_rhs_1931

sc_or_short_1931:
  br label %sc_or_merge_1931

sc_or_rhs_1931:
  %t64_1934 = load i64, i64* %var_st_1927
  %cond_1935 = icmp eq i64 %t64_1934, 0
  br label %sc_or_done_1931

sc_or_done_1931:
  br label %sc_or_merge_1931

sc_or_merge_1931:
  %or_cond_1936 = phi i1 [ true, %sc_or_short_1931 ], [ %cond_1935, %sc_or_done_1931 ]
  br label %sc_and_done_1928

sc_and_done_1928:
  br label %sc_and_merge_1928

sc_and_merge_1928:
  %and_cond_1937 = phi i1 [ false, %sc_and_short_1928 ], [ %or_cond_1936, %sc_and_done_1928 ]
  br i1 %and_cond_1937, label %if_then_66, label %if_merge_66

if_then_66:
  %t64_1938 = load i64, i64* %param_ctx_1786
  %t_1939 = add i64 %t64_1938, 52
  %t64_1940 = load i64, i64* %param_dhcp_1790
  %t_1941 = add i64 %t64_1940, 16
  %fn_ret_1942 = call i64 @__omni_cyw_priv_bcopy(i64 %t_1939, i64 %t_1941, i64 4, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_1943 = load i64, i64* %param_ctx_1786
  %t_1944 = add i64 %t64_1943, 64
  %t64_1945 = load i64, i64* %param_eth_1788
  %t_1946 = add i64 %t64_1945, 6
  %fn_ret_1947 = call i64 @__omni_cyw_priv_bcopy(i64 %t_1944, i64 %t_1946, i64 6, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_1948 = load i64, i64* %var_has_srv_1834
  %cond_1949 = icmp ne i64 %t64_1948, 0
  br i1 %cond_1949, label %if_then_67, label %if_merge_67

if_then_67:
  %t64_1950 = load i64, i64* %param_ctx_1786
  %t_1951 = add i64 %t64_1950, 56
  %t64_1952 = load i64, i64* %var_srv_tmp_1838
  %fn_ret_1953 = call i64 @__omni_cyw_priv_bcopy(i64 %t_1951, i64 %t64_1952, i64 4, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_1954 = load i64, i64* %param_ctx_1786
  %t_1955 = add i64 %t64_1954, 60
  %ext_i64_1956 = call i64 @mmio_write32(i64 %t_1955, i64 1)
  br label %if_merge_67

if_merge_67:
  %t64_1957 = load i64, i64* %param_ctx_1786
  %t_1958 = add i64 %t64_1957, 8
  %ext_i64_1959 = call i64 @mmio_write32(i64 %t_1958, i64 2)
  ret i64 1
if_merge_66:
  %t64_1961 = load i64, i64* %var_msg_type_1833
  %cond_1962 = icmp eq i64 %t64_1961, 5
  br i1 %cond_1962, label %sc_and_rhs_1960, label %sc_and_short_1960

sc_and_short_1960:
  br label %sc_and_merge_1960

sc_and_rhs_1960:
  %t64_1965 = load i64, i64* %var_st_1927
  %cond_1966 = icmp eq i64 %t64_1965, 3
  br i1 %cond_1966, label %sc_or_short_1964, label %sc_or_rhs_1964

sc_or_short_1964:
  br label %sc_or_merge_1964

sc_or_rhs_1964:
  %t64_1967 = load i64, i64* %var_st_1927
  %cond_1968 = icmp eq i64 %t64_1967, 2
  br label %sc_or_done_1964

sc_or_done_1964:
  br label %sc_or_merge_1964

sc_or_merge_1964:
  %or_cond_1969 = phi i1 [ true, %sc_or_short_1964 ], [ %cond_1968, %sc_or_done_1964 ]
  br i1 %or_cond_1969, label %sc_or_short_1963, label %sc_or_rhs_1963

sc_or_short_1963:
  br label %sc_or_merge_1963

sc_or_rhs_1963:
  %t64_1970 = load i64, i64* %var_st_1927
  %cond_1971 = icmp eq i64 %t64_1970, 1
  br label %sc_or_done_1963

sc_or_done_1963:
  br label %sc_or_merge_1963

sc_or_merge_1963:
  %or_cond_1972 = phi i1 [ true, %sc_or_short_1963 ], [ %cond_1971, %sc_or_done_1963 ]
  br label %sc_and_done_1960

sc_and_done_1960:
  br label %sc_and_merge_1960

sc_and_merge_1960:
  %and_cond_1973 = phi i1 [ false, %sc_and_short_1960 ], [ %or_cond_1972, %sc_and_done_1960 ]
  br i1 %and_cond_1973, label %if_then_68, label %if_merge_68

if_then_68:
  %t64_1974 = load i64, i64* %param_ctx_1786
  %t_1975 = add i64 %t64_1974, 40
  %t64_1976 = load i64, i64* %param_dhcp_1790
  %t_1977 = add i64 %t64_1976, 16
  %fn_ret_1978 = call i64 @__omni_cyw_priv_bcopy(i64 %t_1975, i64 %t_1977, i64 4, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_1979 = load i64, i64* %var_sub_tmp_1842
  %fn_ret_1980 = call i64 @__omni_cyw_priv_ip_is_zero(i64 %t64_1979, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %cond_1981 = icmp eq i64 %fn_ret_1980, 0
  br i1 %cond_1981, label %if_then_69, label %if_merge_69

if_then_69:
  %t64_1982 = load i64, i64* %param_ctx_1786
  %t_1983 = add i64 %t64_1982, 44
  %t64_1984 = load i64, i64* %var_sub_tmp_1842
  %fn_ret_1985 = call i64 @__omni_cyw_priv_bcopy(i64 %t_1983, i64 %t64_1984, i64 4, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  br label %if_merge_69

if_merge_69:
  %t64_1986 = load i64, i64* %var_rtr_tmp_1846
  %fn_ret_1987 = call i64 @__omni_cyw_priv_ip_is_zero(i64 %t64_1986, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %cond_1988 = icmp eq i64 %fn_ret_1987, 0
  br i1 %cond_1988, label %if_then_70, label %if_merge_70

if_then_70:
  %t64_1989 = load i64, i64* %param_ctx_1786
  %t_1990 = add i64 %t64_1989, 48
  %t64_1991 = load i64, i64* %var_rtr_tmp_1846
  %fn_ret_1992 = call i64 @__omni_cyw_priv_bcopy(i64 %t_1990, i64 %t64_1991, i64 4, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  br label %if_merge_70

if_merge_70:
  %t64_1993 = load i64, i64* %param_ctx_1786
  %t_1994 = add i64 %t64_1993, 40
  %t64_1995 = load i64, i64* %param_ctx_1786
  %t_1996 = add i64 %t64_1995, 128
  %fn_ret_1997 = call i64 @__omni_cyw_priv_format_ip_str(i64 %t_1994, i64 %t_1996, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_1998 = load i64, i64* %param_ctx_1786
  %t_1999 = add i64 %t64_1998, 8
  %ext_i64_2000 = call i64 @mmio_write32(i64 %t_1999, i64 4)
  ret i64 1
if_merge_68:
  ret i64 0
}


define void @__omni_trampoline_dhcp_rx_handle(i8* %raw_arg) {
entry:
  %raw_int = ptrtoint i8* %raw_arg to i64
  %res = call i64 @__omni_cyw_priv_dhcp_rx_handle(i64 %raw_int, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  ret void
}

define void @__omni_trampoline_pack_dhcp_rx_handle(i8* %raw_pack) {
entry:
  %p64 = bitcast i8* %raw_pack to i64*
  %p0 = getelementptr inbounds i64, i64* %p64, i64 0
  %a0 = load i64, i64* %p0
  %p1 = getelementptr inbounds i64, i64* %p64, i64 1
  %a1 = load i64, i64* %p1
  %p2 = getelementptr inbounds i64, i64* %p64, i64 2
  %a2 = load i64, i64* %p2
  %p3 = getelementptr inbounds i64, i64* %p64, i64 3
  %a3 = load i64, i64* %p3
  %p4 = getelementptr inbounds i64, i64* %p64, i64 4
  %a4 = load i64, i64* %p4
  %p5 = getelementptr inbounds i64, i64* %p64, i64 5
  %a5 = load i64, i64* %p5
  call void @free(i8* %raw_pack)
  %res = call i64 @__omni_cyw_priv_dhcp_rx_handle(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5)
  ret void
}

define i64 @__omni_cyw_priv_tcp_send_segment(i64 %arg0, i64 %arg1, i64 %arg2, i64 %arg3, i64 %arg4, i64 %arg5, i64 %arg6, i64 %arg7, i64 %arg8, i64 %arg9) {
entry:
  %param_ctx_2002 = alloca i64
  %param_dmac_2004 = alloca i64
  %param_dip_2006 = alloca i64
  %param_dport_2008 = alloca i64
  %param_sport_2010 = alloca i64
  %param_seq_2012 = alloca i64
  %param_ack_2014 = alloca i64
  %param_flags_2016 = alloca i64
  %param_data_ptr_2018 = alloca i64
  %param_data_len_2020 = alloca i64
  %var_pkt_2024 = alloca i64
  %var_my_mac_2027 = alloca i64
  %var_my_ip_2030 = alloca i64
  %var_tcp_2033 = alloca i64
  %var_hlen_2034 = alloca i64
  %var_csum_2123 = alloca i64
  %param_unset_ctx_2001 = icmp eq i64 %arg0, -581005069656919554
  %param_dsel_ctx_2001 = select i1 %param_unset_ctx_2001, i64 0, i64 %arg0
  store i64 %param_dsel_ctx_2001, i64* %param_ctx_2002
  %param_unset_dmac_2003 = icmp eq i64 %arg1, -581005069656919554
  %param_dsel_dmac_2003 = select i1 %param_unset_dmac_2003, i64 0, i64 %arg1
  store i64 %param_dsel_dmac_2003, i64* %param_dmac_2004
  %param_unset_dip_2005 = icmp eq i64 %arg2, -581005069656919554
  %param_dsel_dip_2005 = select i1 %param_unset_dip_2005, i64 0, i64 %arg2
  store i64 %param_dsel_dip_2005, i64* %param_dip_2006
  %param_unset_dport_2007 = icmp eq i64 %arg3, -581005069656919554
  %param_dsel_dport_2007 = select i1 %param_unset_dport_2007, i64 0, i64 %arg3
  store i64 %param_dsel_dport_2007, i64* %param_dport_2008
  %param_unset_sport_2009 = icmp eq i64 %arg4, -581005069656919554
  %param_dsel_sport_2009 = select i1 %param_unset_sport_2009, i64 0, i64 %arg4
  store i64 %param_dsel_sport_2009, i64* %param_sport_2010
  %param_unset_seq_2011 = icmp eq i64 %arg5, -581005069656919554
  %param_dsel_seq_2011 = select i1 %param_unset_seq_2011, i64 0, i64 %arg5
  store i64 %param_dsel_seq_2011, i64* %param_seq_2012
  %param_unset_ack_2013 = icmp eq i64 %arg6, -581005069656919554
  %param_dsel_ack_2013 = select i1 %param_unset_ack_2013, i64 0, i64 %arg6
  store i64 %param_dsel_ack_2013, i64* %param_ack_2014
  %param_unset_flags_2015 = icmp eq i64 %arg7, -581005069656919554
  %param_dsel_flags_2015 = select i1 %param_unset_flags_2015, i64 0, i64 %arg7
  store i64 %param_dsel_flags_2015, i64* %param_flags_2016
  %param_unset_data_ptr_2017 = icmp eq i64 %arg8, -581005069656919554
  %param_dsel_data_ptr_2017 = select i1 %param_unset_data_ptr_2017, i64 0, i64 %arg8
  store i64 %param_dsel_data_ptr_2017, i64* %param_data_ptr_2018
  %param_unset_data_len_2019 = icmp eq i64 %arg9, -581005069656919554
  %param_dsel_data_len_2019 = select i1 %param_unset_data_len_2019, i64 0, i64 %arg9
  store i64 %param_dsel_data_len_2019, i64* %param_data_len_2020
  call void @__omni_c_return_ptr(i8* null)
  %t64_2021 = load i64, i64* %param_ctx_2002
  %t_2022 = add i64 %t64_2021, 4672
  %t_2023 = add i64 %t_2022, 18
  store i64 %t_2023, i64* %var_pkt_2024
  call void @__omni_c_return_ptr(i8* null)
  %t64_2025 = load i64, i64* %param_ctx_2002
  %t_2026 = add i64 %t64_2025, 32
  store i64 %t_2026, i64* %var_my_mac_2027
  call void @__omni_c_return_ptr(i8* null)
  %t64_2028 = load i64, i64* %param_ctx_2002
  %t_2029 = add i64 %t64_2028, 40
  store i64 %t_2029, i64* %var_my_ip_2030
  call void @__omni_c_return_ptr(i8* null)
  %t64_2031 = load i64, i64* %var_pkt_2024
  %t_2032 = add i64 %t64_2031, 34
  store i64 %t_2032, i64* %var_tcp_2033
  call void @__omni_c_return_ptr(i8* null)
  store i64 20, i64* %var_hlen_2034
  %t64_2035 = load i64, i64* %param_flags_2016
  %t_2036 = and i64 %t64_2035, 2
  %cond_2037 = icmp ne i64 %t_2036, 0
  br i1 %cond_2037, label %if_then_71, label %if_merge_71

if_then_71:
  %t64_2038 = load i64, i64* %var_tcp_2033
  %t_2039 = add i64 %t64_2038, 20
  %fn_ret_2040 = call i64 @__omni_cyw_priv_w8(i64 %t_2039, i64 2, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_2041 = load i64, i64* %var_tcp_2033
  %t_2042 = add i64 %t64_2041, 21
  %fn_ret_2043 = call i64 @__omni_cyw_priv_w8(i64 %t_2042, i64 4, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_2044 = load i64, i64* %var_tcp_2033
  %t_2045 = add i64 %t64_2044, 22
  %fn_ret_2046 = call i64 @__omni_cyw_priv_w16_be(i64 %t_2045, i64 1400, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  call void @__omni_c_return_ptr(i8* null)
  store i64 24, i64* %var_hlen_2034
  br label %if_merge_71

if_merge_71:
  %t64_2049 = load i64, i64* %param_data_len_2020
  %cond_2050 = icmp sgt i64 %t64_2049, 0
  br i1 %cond_2050, label %sc_and_rhs_2048, label %sc_and_short_2048

sc_and_short_2048:
  br label %sc_and_merge_2048

sc_and_rhs_2048:
  %t64_2051 = load i64, i64* %param_data_ptr_2018
  %cond_2052 = icmp ne i64 %t64_2051, 0
  br label %sc_and_done_2048

sc_and_done_2048:
  br label %sc_and_merge_2048

sc_and_merge_2048:
  %and_cond_2053 = phi i1 [ false, %sc_and_short_2048 ], [ %cond_2052, %sc_and_done_2048 ]
  br i1 %and_cond_2053, label %sc_and_rhs_2047, label %sc_and_short_2047

sc_and_short_2047:
  br label %sc_and_merge_2047

sc_and_rhs_2047:
  %t64_2054 = load i64, i64* %param_data_ptr_2018
  %t64_2055 = load i64, i64* %var_tcp_2033
  %t64_2056 = load i64, i64* %var_hlen_2034
  %t_2057 = add i64 %t64_2055, %t64_2056
  %cond_2058 = icmp ne i64 %t64_2054, %t_2057
  br label %sc_and_done_2047

sc_and_done_2047:
  br label %sc_and_merge_2047

sc_and_merge_2047:
  %and_cond_2059 = phi i1 [ false, %sc_and_short_2047 ], [ %cond_2058, %sc_and_done_2047 ]
  br i1 %and_cond_2059, label %if_then_72, label %if_merge_72

if_then_72:
  %t64_2060 = load i64, i64* %var_tcp_2033
  %t64_2061 = load i64, i64* %var_hlen_2034
  %t_2062 = add i64 %t64_2060, %t64_2061
  %t64_2063 = load i64, i64* %param_data_ptr_2018
  %t64_2064 = load i64, i64* %param_data_len_2020
  %fn_ret_2065 = call i64 @__omni_cyw_priv_bcopy(i64 %t_2062, i64 %t64_2063, i64 %t64_2064, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  br label %if_merge_72

if_merge_72:
  %t64_2066 = load i64, i64* %var_pkt_2024
  %t64_2067 = load i64, i64* %param_dmac_2004
  %t64_2068 = load i64, i64* %var_my_mac_2027
  %fn_ret_2069 = call i64 @__omni_cyw_priv_build_eth_hdr(i64 %t64_2066, i64 %t64_2067, i64 %t64_2068, i64 2048, i64 -581005069656919554, i64 -581005069656919554)
  %t64_2070 = load i64, i64* %param_ctx_2002
  %t64_2071 = load i64, i64* %var_pkt_2024
  %t_2072 = add i64 %t64_2071, 14
  %t64_2073 = load i64, i64* %var_my_ip_2030
  %t64_2074 = load i64, i64* %param_dip_2006
  %t64_2075 = load i64, i64* %var_hlen_2034
  %t64_2076 = load i64, i64* %param_data_len_2020
  %t_2077 = add i64 %t64_2075, %t64_2076
  %fn_ret_2078 = call i64 @__omni_cyw_priv_build_ipv4_hdr(i64 %t64_2070, i64 %t_2072, i64 %t64_2073, i64 %t64_2074, i64 6, i64 %t_2077)
  %t64_2079 = load i64, i64* %var_tcp_2033
  %t_2080 = add i64 %t64_2079, 0
  %t64_2081 = load i64, i64* %param_sport_2010
  %fn_ret_2082 = call i64 @__omni_cyw_priv_w16_be(i64 %t_2080, i64 %t64_2081, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_2083 = load i64, i64* %var_tcp_2033
  %t_2084 = add i64 %t64_2083, 2
  %t64_2085 = load i64, i64* %param_dport_2008
  %fn_ret_2086 = call i64 @__omni_cyw_priv_w16_be(i64 %t_2084, i64 %t64_2085, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_2087 = load i64, i64* %var_tcp_2033
  %t_2088 = add i64 %t64_2087, 4
  %t64_2089 = load i64, i64* %param_seq_2012
  %fn_ret_2090 = call i64 @__omni_cyw_priv_w32_be(i64 %t_2088, i64 %t64_2089, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_2091 = load i64, i64* %var_tcp_2033
  %t_2092 = add i64 %t64_2091, 8
  %t64_2093 = load i64, i64* %param_ack_2014
  %fn_ret_2094 = call i64 @__omni_cyw_priv_w32_be(i64 %t_2092, i64 %t64_2093, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_2095 = load i64, i64* %var_tcp_2033
  %t_2096 = add i64 %t64_2095, 12
  %t64_2097 = load i64, i64* %var_hlen_2034
  %div_zero_2099 = icmp eq i64 4, 0
  %safe_div_2099 = select i1 %div_zero_2099, i64 1, i64 4
  %t_2099 = sdiv i64 %t64_2097, %safe_div_2099
  %t_2100 = shl i64 %t_2099, 4
  %fn_ret_2101 = call i64 @__omni_cyw_priv_w8(i64 %t_2096, i64 %t_2100, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_2102 = load i64, i64* %var_tcp_2033
  %t_2103 = add i64 %t64_2102, 13
  %t64_2104 = load i64, i64* %param_flags_2016
  %t_2105 = and i64 %t64_2104, 63
  %fn_ret_2106 = call i64 @__omni_cyw_priv_w8(i64 %t_2103, i64 %t_2105, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_2107 = load i64, i64* %var_tcp_2033
  %t_2108 = add i64 %t64_2107, 14
  %fn_ret_2109 = call i64 @__omni_cyw_priv_w16_be(i64 %t_2108, i64 4096, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_2110 = load i64, i64* %var_tcp_2033
  %t_2111 = add i64 %t64_2110, 16
  %fn_ret_2112 = call i64 @__omni_cyw_priv_w16_be(i64 %t_2111, i64 0, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_2113 = load i64, i64* %var_tcp_2033
  %t_2114 = add i64 %t64_2113, 18
  %fn_ret_2115 = call i64 @__omni_cyw_priv_w16_be(i64 %t_2114, i64 0, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  call void @__omni_c_return_ptr(i8* null)
  %t64_2116 = load i64, i64* %var_my_ip_2030
  %t64_2117 = load i64, i64* %param_dip_2006
  %t64_2118 = load i64, i64* %var_tcp_2033
  %t64_2119 = load i64, i64* %var_hlen_2034
  %t64_2120 = load i64, i64* %param_data_len_2020
  %t_2121 = add i64 %t64_2119, %t64_2120
  %fn_ret_2122 = call i64 @__omni_cyw_priv_l4_checksum(i64 %t64_2116, i64 %t64_2117, i64 6, i64 %t64_2118, i64 %t_2121, i64 -581005069656919554)
  store i64 %fn_ret_2122, i64* %var_csum_2123
  %t64_2124 = load i64, i64* %var_tcp_2033
  %t_2125 = add i64 %t64_2124, 16
  %t64_2126 = load i64, i64* %var_csum_2123
  %fn_ret_2127 = call i64 @__omni_cyw_priv_w16_be(i64 %t_2125, i64 %t64_2126, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_2128 = load i64, i64* %param_ctx_2002
  %t64_2129 = load i64, i64* %var_pkt_2024
  %t64_2130 = load i64, i64* %var_hlen_2034
  %t_2131 = add i64 34, %t64_2130
  %t64_2132 = load i64, i64* %param_data_len_2020
  %t_2133 = add i64 %t_2131, %t64_2132
  %fn_ret_2134 = call i64 @__omni_fn_cyw_tx_eth(i64 %t64_2128, i64 %t64_2129, i64 %t_2133, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  ret i64 %fn_ret_2134
}


define void @__omni_trampoline_tcp_send_segment(i8* %raw_arg) {
entry:
  %raw_int = ptrtoint i8* %raw_arg to i64
  %res = call i64 @__omni_cyw_priv_tcp_send_segment(i64 %raw_int, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  ret void
}

define void @__omni_trampoline_pack_tcp_send_segment(i8* %raw_pack) {
entry:
  %p64 = bitcast i8* %raw_pack to i64*
  %p0 = getelementptr inbounds i64, i64* %p64, i64 0
  %a0 = load i64, i64* %p0
  %p1 = getelementptr inbounds i64, i64* %p64, i64 1
  %a1 = load i64, i64* %p1
  %p2 = getelementptr inbounds i64, i64* %p64, i64 2
  %a2 = load i64, i64* %p2
  %p3 = getelementptr inbounds i64, i64* %p64, i64 3
  %a3 = load i64, i64* %p3
  %p4 = getelementptr inbounds i64, i64* %p64, i64 4
  %a4 = load i64, i64* %p4
  %p5 = getelementptr inbounds i64, i64* %p64, i64 5
  %a5 = load i64, i64* %p5
  call void @free(i8* %raw_pack)
  %res = call i64 @__omni_cyw_priv_tcp_send_segment(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  ret void
}

define i64 @__omni_cyw_priv_tcp_send_tracked(i64 %arg0, i64 %arg1, i64 %arg2, i64 %arg3, i64 %arg4, i64 %arg5) {
entry:
  %param_ctx_2136 = alloca i64
  %param_flags_2138 = alloca i64
  %param_data_ptr_2140 = alloca i64
  %param_data_len_2142 = alloca i64
  %var_seq_2146 = alloca i64
  %var_ack_2150 = alloca i64
  %var_retx_2153 = alloca i64
  %var_adv_2185 = alloca i64
  %param_unset_ctx_2135 = icmp eq i64 %arg0, -581005069656919554
  %param_dsel_ctx_2135 = select i1 %param_unset_ctx_2135, i64 0, i64 %arg0
  store i64 %param_dsel_ctx_2135, i64* %param_ctx_2136
  %param_unset_flags_2137 = icmp eq i64 %arg1, -581005069656919554
  %param_dsel_flags_2137 = select i1 %param_unset_flags_2137, i64 0, i64 %arg1
  store i64 %param_dsel_flags_2137, i64* %param_flags_2138
  %param_unset_data_ptr_2139 = icmp eq i64 %arg2, -581005069656919554
  %param_dsel_data_ptr_2139 = select i1 %param_unset_data_ptr_2139, i64 0, i64 %arg2
  store i64 %param_dsel_data_ptr_2139, i64* %param_data_ptr_2140
  %param_unset_data_len_2141 = icmp eq i64 %arg3, -581005069656919554
  %param_dsel_data_len_2141 = select i1 %param_unset_data_len_2141, i64 0, i64 %arg3
  store i64 %param_dsel_data_len_2141, i64* %param_data_len_2142
  call void @__omni_c_return_ptr(i8* null)
  %t64_2143 = load i64, i64* %param_ctx_2136
  %t_2144 = add i64 %t64_2143, 88
  %ext_i64_2145 = call i64 @mmio_read32(i64 %t_2144)
  store i64 %ext_i64_2145, i64* %var_seq_2146
  call void @__omni_c_return_ptr(i8* null)
  %t64_2147 = load i64, i64* %param_ctx_2136
  %t_2148 = add i64 %t64_2147, 92
  %ext_i64_2149 = call i64 @mmio_read32(i64 %t_2148)
  store i64 %ext_i64_2149, i64* %var_ack_2150
  call void @__omni_c_return_ptr(i8* null)
  %t64_2151 = load i64, i64* %param_ctx_2136
  %t_2152 = add i64 %t64_2151, 6464
  store i64 %t_2152, i64* %var_retx_2153
  %t64_2155 = load i64, i64* %param_data_len_2142
  %cond_2156 = icmp sgt i64 %t64_2155, 0
  br i1 %cond_2156, label %sc_and_rhs_2154, label %sc_and_short_2154

sc_and_short_2154:
  br label %sc_and_merge_2154

sc_and_rhs_2154:
  %t64_2157 = load i64, i64* %param_data_ptr_2140
  %t64_2158 = load i64, i64* %var_retx_2153
  %cond_2159 = icmp ne i64 %t64_2157, %t64_2158
  br label %sc_and_done_2154

sc_and_done_2154:
  br label %sc_and_merge_2154

sc_and_merge_2154:
  %and_cond_2160 = phi i1 [ false, %sc_and_short_2154 ], [ %cond_2159, %sc_and_done_2154 ]
  br i1 %and_cond_2160, label %if_then_73, label %if_merge_73

if_then_73:
  %t64_2161 = load i64, i64* %var_retx_2153
  %t64_2162 = load i64, i64* %param_data_ptr_2140
  %t64_2163 = load i64, i64* %param_data_len_2142
  %fn_ret_2164 = call i64 @__omni_cyw_priv_bcopy(i64 %t64_2161, i64 %t64_2162, i64 %t64_2163, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  br label %if_merge_73

if_merge_73:
  %t64_2165 = load i64, i64* %param_ctx_2136
  %t_2166 = add i64 %t64_2165, 96
  %t64_2167 = load i64, i64* %var_seq_2146
  %ext_i64_2168 = call i64 @mmio_write32(i64 %t_2166, i64 %t64_2167)
  %t64_2169 = load i64, i64* %param_ctx_2136
  %t_2170 = add i64 %t64_2169, 100
  %t64_2171 = load i64, i64* %param_data_len_2142
  %ext_i64_2172 = call i64 @mmio_write32(i64 %t_2170, i64 %t64_2171)
  %t64_2173 = load i64, i64* %param_ctx_2136
  %t_2174 = add i64 %t64_2173, 104
  %t64_2175 = load i64, i64* %param_flags_2138
  %ext_i64_2176 = call i64 @mmio_write32(i64 %t_2174, i64 %t64_2175)
  %t64_2177 = load i64, i64* %param_ctx_2136
  %t_2178 = add i64 %t64_2177, 108
  %fn_ret_2179 = call i64 @__omni_cyw_priv_now_ms(i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %ext_i64_2180 = call i64 @mmio_write32(i64 %t_2178, i64 %fn_ret_2179)
  %t64_2181 = load i64, i64* %param_ctx_2136
  %t_2182 = add i64 %t64_2181, 112
  %ext_i64_2183 = call i64 @mmio_write32(i64 %t_2182, i64 0)
  call void @__omni_c_return_ptr(i8* null)
  %t64_2184 = load i64, i64* %param_data_len_2142
  store i64 %t64_2184, i64* %var_adv_2185
  %t64_2186 = load i64, i64* %param_flags_2138
  %t_2187 = and i64 %t64_2186, 3
  %cond_2188 = icmp ne i64 %t_2187, 0
  br i1 %cond_2188, label %if_then_74, label %if_merge_74

if_then_74:
  %t64_2189 = load i64, i64* %var_adv_2185
  %cond_2190 = icmp eq i64 %t64_2189, 0
  br i1 %cond_2190, label %if_then_75, label %if_merge_75

if_then_75:
  call void @__omni_c_return_ptr(i8* null)
  store i64 1, i64* %var_adv_2185
  br label %if_merge_75

if_merge_75:
  br label %if_merge_74

if_merge_74:
  %t64_2191 = load i64, i64* %param_ctx_2136
  %t_2192 = add i64 %t64_2191, 88
  %t64_2193 = load i64, i64* %var_seq_2146
  %t64_2194 = load i64, i64* %var_adv_2185
  %t_2195 = add i64 %t64_2193, %t64_2194
  %t_2196 = and i64 %t_2195, 4294967295
  %ext_i64_2197 = call i64 @mmio_write32(i64 %t_2192, i64 %t_2196)
  %t64_2198 = load i64, i64* %param_ctx_2136
  %t64_2199 = load i64, i64* %param_ctx_2136
  %t_2200 = add i64 %t64_2199, 72
  %t64_2201 = load i64, i64* %param_ctx_2136
  %t_2202 = add i64 %t64_2201, 80
  %t64_2203 = load i64, i64* %param_ctx_2136
  %t_2204 = add i64 %t64_2203, 84
  %ext_i64_2205 = call i64 @mmio_read32(i64 %t_2204)
  %t64_2206 = load i64, i64* %param_ctx_2136
  %t_2207 = add i64 %t64_2206, 16
  %ext_i64_2208 = call i64 @mmio_read32(i64 %t_2207)
  %t64_2209 = load i64, i64* %var_seq_2146
  %t64_2210 = load i64, i64* %var_ack_2150
  %t64_2211 = load i64, i64* %param_flags_2138
  %t64_2212 = load i64, i64* %var_retx_2153
  %t64_2213 = load i64, i64* %param_data_len_2142
  %fn_ret_2214 = call i64 @__omni_cyw_priv_tcp_send_segment(i64 %t64_2198, i64 %t_2200, i64 %t_2202, i64 %ext_i64_2205, i64 %ext_i64_2208, i64 %t64_2209, i64 %t64_2210, i64 %t64_2211, i64 %t64_2212, i64 %t64_2213)
  ret i64 %fn_ret_2214
}


define void @__omni_trampoline_tcp_send_tracked(i8* %raw_arg) {
entry:
  %raw_int = ptrtoint i8* %raw_arg to i64
  %res = call i64 @__omni_cyw_priv_tcp_send_tracked(i64 %raw_int, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  ret void
}

define void @__omni_trampoline_pack_tcp_send_tracked(i8* %raw_pack) {
entry:
  %p64 = bitcast i8* %raw_pack to i64*
  %p0 = getelementptr inbounds i64, i64* %p64, i64 0
  %a0 = load i64, i64* %p0
  %p1 = getelementptr inbounds i64, i64* %p64, i64 1
  %a1 = load i64, i64* %p1
  %p2 = getelementptr inbounds i64, i64* %p64, i64 2
  %a2 = load i64, i64* %p2
  %p3 = getelementptr inbounds i64, i64* %p64, i64 3
  %a3 = load i64, i64* %p3
  %p4 = getelementptr inbounds i64, i64* %p64, i64 4
  %a4 = load i64, i64* %p4
  %p5 = getelementptr inbounds i64, i64* %p64, i64 5
  %a5 = load i64, i64* %p5
  call void @free(i8* %raw_pack)
  %res = call i64 @__omni_cyw_priv_tcp_send_tracked(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5)
  ret void
}

define i64 @__omni_cyw_priv_tcp_send_pure_ack(i64 %arg0, i64 %arg1, i64 %arg2, i64 %arg3, i64 %arg4, i64 %arg5) {
entry:
  %param_ctx_2216 = alloca i64
  %param_unset_ctx_2215 = icmp eq i64 %arg0, -581005069656919554
  %param_dsel_ctx_2215 = select i1 %param_unset_ctx_2215, i64 0, i64 %arg0
  store i64 %param_dsel_ctx_2215, i64* %param_ctx_2216
  %t64_2217 = load i64, i64* %param_ctx_2216
  %t64_2218 = load i64, i64* %param_ctx_2216
  %t_2219 = add i64 %t64_2218, 72
  %t64_2220 = load i64, i64* %param_ctx_2216
  %t_2221 = add i64 %t64_2220, 80
  %t64_2222 = load i64, i64* %param_ctx_2216
  %t_2223 = add i64 %t64_2222, 84
  %ext_i64_2224 = call i64 @mmio_read32(i64 %t_2223)
  %t64_2225 = load i64, i64* %param_ctx_2216
  %t_2226 = add i64 %t64_2225, 16
  %ext_i64_2227 = call i64 @mmio_read32(i64 %t_2226)
  %t64_2228 = load i64, i64* %param_ctx_2216
  %t_2229 = add i64 %t64_2228, 88
  %ext_i64_2230 = call i64 @mmio_read32(i64 %t_2229)
  %t64_2231 = load i64, i64* %param_ctx_2216
  %t_2232 = add i64 %t64_2231, 92
  %ext_i64_2233 = call i64 @mmio_read32(i64 %t_2232)
  %fn_ret_2234 = call i64 @__omni_cyw_priv_tcp_send_segment(i64 %t64_2217, i64 %t_2219, i64 %t_2221, i64 %ext_i64_2224, i64 %ext_i64_2227, i64 %ext_i64_2230, i64 %ext_i64_2233, i64 16, i64 0, i64 0)
  ret i64 %fn_ret_2234
}


define void @__omni_trampoline_tcp_send_pure_ack(i8* %raw_arg) {
entry:
  %raw_int = ptrtoint i8* %raw_arg to i64
  %res = call i64 @__omni_cyw_priv_tcp_send_pure_ack(i64 %raw_int, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  ret void
}

define void @__omni_trampoline_pack_tcp_send_pure_ack(i8* %raw_pack) {
entry:
  %p64 = bitcast i8* %raw_pack to i64*
  %p0 = getelementptr inbounds i64, i64* %p64, i64 0
  %a0 = load i64, i64* %p0
  %p1 = getelementptr inbounds i64, i64* %p64, i64 1
  %a1 = load i64, i64* %p1
  %p2 = getelementptr inbounds i64, i64* %p64, i64 2
  %a2 = load i64, i64* %p2
  %p3 = getelementptr inbounds i64, i64* %p64, i64 3
  %a3 = load i64, i64* %p3
  %p4 = getelementptr inbounds i64, i64* %p64, i64 4
  %a4 = load i64, i64* %p4
  %p5 = getelementptr inbounds i64, i64* %p64, i64 5
  %a5 = load i64, i64* %p5
  call void @free(i8* %raw_pack)
  %res = call i64 @__omni_cyw_priv_tcp_send_pure_ack(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5)
  ret void
}

define i64 @__omni_cyw_priv_ws_send_raw_frame(i64 %arg0, i64 %arg1, i64 %arg2, i64 %arg3, i64 %arg4, i64 %arg5) {
entry:
  %param_ctx_2236 = alloca i64
  %param_opcode_2238 = alloca i64
  %param_payload_ptr_2240 = alloca i64
  %param_payload_len_2242 = alloca i64
  %var_retx_2249 = alloca i64
  %var_plen_2251 = alloca i64
  %var_hdr_2260 = alloca i64
  %param_unset_ctx_2235 = icmp eq i64 %arg0, -581005069656919554
  %param_dsel_ctx_2235 = select i1 %param_unset_ctx_2235, i64 0, i64 %arg0
  store i64 %param_dsel_ctx_2235, i64* %param_ctx_2236
  %param_unset_opcode_2237 = icmp eq i64 %arg1, -581005069656919554
  %param_dsel_opcode_2237 = select i1 %param_unset_opcode_2237, i64 0, i64 %arg1
  store i64 %param_dsel_opcode_2237, i64* %param_opcode_2238
  %param_unset_payload_ptr_2239 = icmp eq i64 %arg2, -581005069656919554
  %param_dsel_payload_ptr_2239 = select i1 %param_unset_payload_ptr_2239, i64 0, i64 %arg2
  store i64 %param_dsel_payload_ptr_2239, i64* %param_payload_ptr_2240
  %param_unset_payload_len_2241 = icmp eq i64 %arg3, -581005069656919554
  %param_dsel_payload_len_2241 = select i1 %param_unset_payload_len_2241, i64 0, i64 %arg3
  store i64 %param_dsel_payload_len_2241, i64* %param_payload_len_2242
  %t64_2243 = load i64, i64* %param_ctx_2236
  %t_2244 = add i64 %t64_2243, 12
  %ext_i64_2245 = call i64 @mmio_read32(i64 %t_2244)
  %cond_2246 = icmp ne i64 %ext_i64_2245, 4
  br i1 %cond_2246, label %if_then_76, label %if_merge_76

if_then_76:
  ret i64 -1
if_merge_76:
  call void @__omni_c_return_ptr(i8* null)
  %t64_2247 = load i64, i64* %param_ctx_2236
  %t_2248 = add i64 %t64_2247, 6464
  store i64 %t_2248, i64* %var_retx_2249
  call void @__omni_c_return_ptr(i8* null)
  %t64_2250 = load i64, i64* %param_payload_len_2242
  store i64 %t64_2250, i64* %var_plen_2251
  %t64_2252 = load i64, i64* %var_plen_2251
  %cond_2253 = icmp sgt i64 %t64_2252, 1380
  br i1 %cond_2253, label %if_then_77, label %if_merge_77

if_then_77:
  call void @__omni_c_return_ptr(i8* null)
  store i64 1380, i64* %var_plen_2251
  br label %if_merge_77

if_merge_77:
  %t64_2254 = load i64, i64* %var_retx_2249
  %t_2255 = add i64 %t64_2254, 0
  %t64_2256 = load i64, i64* %param_opcode_2238
  %t_2257 = and i64 %t64_2256, 15
  %t_2258 = or i64 128, %t_2257
  %fn_ret_2259 = call i64 @__omni_cyw_priv_w8(i64 %t_2255, i64 %t_2258, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  call void @__omni_c_return_ptr(i8* null)
  store i64 2, i64* %var_hdr_2260
  %t64_2261 = load i64, i64* %var_plen_2251
  %cond_2262 = icmp sle i64 %t64_2261, 125
  br i1 %cond_2262, label %if_then_78, label %if_else_78

if_then_78:
  %t64_2263 = load i64, i64* %var_retx_2249
  %t_2264 = add i64 %t64_2263, 1
  %t64_2265 = load i64, i64* %var_plen_2251
  %fn_ret_2266 = call i64 @__omni_cyw_priv_w8(i64 %t_2264, i64 %t64_2265, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  call void @__omni_c_return_ptr(i8* null)
  store i64 2, i64* %var_hdr_2260
  br label %if_merge_78

if_else_78:
  %t64_2267 = load i64, i64* %var_retx_2249
  %t_2268 = add i64 %t64_2267, 1
  %fn_ret_2269 = call i64 @__omni_cyw_priv_w8(i64 %t_2268, i64 126, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_2270 = load i64, i64* %var_retx_2249
  %t_2271 = add i64 %t64_2270, 2
  %t64_2272 = load i64, i64* %var_plen_2251
  %fn_ret_2273 = call i64 @__omni_cyw_priv_w16_be(i64 %t_2271, i64 %t64_2272, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  call void @__omni_c_return_ptr(i8* null)
  store i64 4, i64* %var_hdr_2260
  br label %if_merge_78

if_merge_78:
  %t64_2275 = load i64, i64* %var_plen_2251
  %cond_2276 = icmp sgt i64 %t64_2275, 0
  br i1 %cond_2276, label %sc_and_rhs_2274, label %sc_and_short_2274

sc_and_short_2274:
  br label %sc_and_merge_2274

sc_and_rhs_2274:
  %t64_2277 = load i64, i64* %param_payload_ptr_2240
  %cond_2278 = icmp ne i64 %t64_2277, 0
  br label %sc_and_done_2274

sc_and_done_2274:
  br label %sc_and_merge_2274

sc_and_merge_2274:
  %and_cond_2279 = phi i1 [ false, %sc_and_short_2274 ], [ %cond_2278, %sc_and_done_2274 ]
  br i1 %and_cond_2279, label %if_then_79, label %if_merge_79

if_then_79:
  %t64_2280 = load i64, i64* %var_retx_2249
  %t64_2281 = load i64, i64* %var_hdr_2260
  %t_2282 = add i64 %t64_2280, %t64_2281
  %t64_2283 = load i64, i64* %param_payload_ptr_2240
  %t64_2284 = load i64, i64* %var_plen_2251
  %fn_ret_2285 = call i64 @__omni_cyw_priv_bcopy(i64 %t_2282, i64 %t64_2283, i64 %t64_2284, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  br label %if_merge_79

if_merge_79:
  %t64_2286 = load i64, i64* %param_ctx_2236
  %t64_2287 = load i64, i64* %var_retx_2249
  %t64_2288 = load i64, i64* %var_hdr_2260
  %t64_2289 = load i64, i64* %var_plen_2251
  %t_2290 = add i64 %t64_2288, %t64_2289
  %fn_ret_2291 = call i64 @__omni_cyw_priv_tcp_send_tracked(i64 %t64_2286, i64 24, i64 %t64_2287, i64 %t_2290, i64 -581005069656919554, i64 -581005069656919554)
  %t64_2292 = load i64, i64* %var_plen_2251
  ret i64 %t64_2292
}


define void @__omni_trampoline_ws_send_raw_frame(i8* %raw_arg) {
entry:
  %raw_int = ptrtoint i8* %raw_arg to i64
  %res = call i64 @__omni_cyw_priv_ws_send_raw_frame(i64 %raw_int, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  ret void
}

define void @__omni_trampoline_pack_ws_send_raw_frame(i8* %raw_pack) {
entry:
  %p64 = bitcast i8* %raw_pack to i64*
  %p0 = getelementptr inbounds i64, i64* %p64, i64 0
  %a0 = load i64, i64* %p0
  %p1 = getelementptr inbounds i64, i64* %p64, i64 1
  %a1 = load i64, i64* %p1
  %p2 = getelementptr inbounds i64, i64* %p64, i64 2
  %a2 = load i64, i64* %p2
  %p3 = getelementptr inbounds i64, i64* %p64, i64 3
  %a3 = load i64, i64* %p3
  %p4 = getelementptr inbounds i64, i64* %p64, i64 4
  %a4 = load i64, i64* %p4
  %p5 = getelementptr inbounds i64, i64* %p64, i64 5
  %a5 = load i64, i64* %p5
  call void @free(i8* %raw_pack)
  %res = call i64 @__omni_cyw_priv_ws_send_raw_frame(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5)
  ret void
}

define i64 @__omni_cyw_priv_ascii_lower(i64 %arg0, i64 %arg1, i64 %arg2, i64 %arg3, i64 %arg4, i64 %arg5) {
entry:
  %param_ch_2295 = alloca i8*
  store i8* null, i8** %param_ch_2295
  %param_unset_ch_2293 = icmp eq i64 %arg0, -581005069656919554
  %param_dsel_ch_2293 = select i1 %param_unset_ch_2293, i64 0, i64 %arg0
  %ptr_cast_ch_2294 = inttoptr i64 %param_dsel_ch_2293 to i8*
  store i8* %ptr_cast_ch_2294, i8** %param_ch_2295
  %ptr_ld_2297 = load i8*, i8** %param_ch_2295
  %p2i64_2299 = ptrtoint i8* %ptr_ld_2297 to i64
  %cond_2298 = icmp sge i64 %p2i64_2299, 65
  br i1 %cond_2298, label %sc_and_rhs_2296, label %sc_and_short_2296

sc_and_short_2296:
  br label %sc_and_merge_2296

sc_and_rhs_2296:
  %ptr_ld_2300 = load i8*, i8** %param_ch_2295
  %p2i64_2302 = ptrtoint i8* %ptr_ld_2300 to i64
  %cond_2301 = icmp sle i64 %p2i64_2302, 90
  br label %sc_and_done_2296

sc_and_done_2296:
  br label %sc_and_merge_2296

sc_and_merge_2296:
  %and_cond_2303 = phi i1 [ false, %sc_and_short_2296 ], [ %cond_2301, %sc_and_done_2296 ]
  br i1 %and_cond_2303, label %if_then_80, label %if_merge_80

if_then_80:
  %ptr_ld_2304 = load i8*, i8** %param_ch_2295
  %p2i64_2305 = ptrtoint i8* %ptr_ld_2304 to i64
  %t_2306 = add i64 %p2i64_2305, 32
  ret i64 %t_2306
if_merge_80:
  %ret_ptr_ld_2307 = load i8*, i8** %param_ch_2295
  call void @__omni_c_return_ptr(i8* %ret_ptr_ld_2307)
  %ret_ptr_i64_2308 = ptrtoint i8* %ret_ptr_ld_2307 to i64
  ret i64 %ret_ptr_i64_2308
}


define void @__omni_trampoline_ascii_lower(i8* %raw_arg) {
entry:
  %raw_int = ptrtoint i8* %raw_arg to i64
  %res = call i64 @__omni_cyw_priv_ascii_lower(i64 %raw_int, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  ret void
}

define void @__omni_trampoline_pack_ascii_lower(i8* %raw_pack) {
entry:
  %p64 = bitcast i8* %raw_pack to i64*
  %p0 = getelementptr inbounds i64, i64* %p64, i64 0
  %a0 = load i64, i64* %p0
  %p1 = getelementptr inbounds i64, i64* %p64, i64 1
  %a1 = load i64, i64* %p1
  %p2 = getelementptr inbounds i64, i64* %p64, i64 2
  %a2 = load i64, i64* %p2
  %p3 = getelementptr inbounds i64, i64* %p64, i64 3
  %a3 = load i64, i64* %p3
  %p4 = getelementptr inbounds i64, i64* %p64, i64 4
  %a4 = load i64, i64* %p4
  %p5 = getelementptr inbounds i64, i64* %p64, i64 5
  %a5 = load i64, i64* %p5
  call void @free(i8* %raw_pack)
  %res = call i64 @__omni_cyw_priv_ascii_lower(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5)
  ret void
}

define i64 @__omni_cyw_priv_http_find_header_end(i64 %arg0, i64 %arg1, i64 %arg2, i64 %arg3, i64 %arg4, i64 %arg5) {
entry:
  %param_buf_2310 = alloca i64
  %param_len_2312 = alloca i64
  %var_i_2313 = alloca i64
  %param_unset_buf_2309 = icmp eq i64 %arg0, -581005069656919554
  %param_dsel_buf_2309 = select i1 %param_unset_buf_2309, i64 0, i64 %arg0
  store i64 %param_dsel_buf_2309, i64* %param_buf_2310
  %param_unset_len_2311 = icmp eq i64 %arg1, -581005069656919554
  %param_dsel_len_2311 = select i1 %param_unset_len_2311, i64 0, i64 %arg1
  store i64 %param_dsel_len_2311, i64* %param_len_2312
  call void @__omni_c_return_ptr(i8* null)
  store i64 0, i64* %var_i_2313
  br label %while_cond_81

while_cond_81:
  %t64_2314 = load i64, i64* %var_i_2313
  %t_2315 = add i64 %t64_2314, 3
  %t64_2316 = load i64, i64* %param_len_2312
  %cond_2317 = icmp slt i64 %t_2315, %t64_2316
  br i1 %cond_2317, label %while_body_81, label %while_exit_81

while_body_81:
  %t64_2321 = load i64, i64* %param_buf_2310
  %t64_2322 = load i64, i64* %var_i_2313
  %t_2323 = add i64 %t64_2321, %t64_2322
  %fn_ret_2324 = call i64 @__omni_cyw_priv_r8(i64 %t_2323, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %cond_2325 = icmp eq i64 %fn_ret_2324, 13
  br i1 %cond_2325, label %sc_and_rhs_2320, label %sc_and_short_2320

sc_and_short_2320:
  br label %sc_and_merge_2320

sc_and_rhs_2320:
  %t64_2326 = load i64, i64* %param_buf_2310
  %t64_2327 = load i64, i64* %var_i_2313
  %t_2328 = add i64 %t64_2326, %t64_2327
  %t_2329 = add i64 %t_2328, 1
  %fn_ret_2330 = call i64 @__omni_cyw_priv_r8(i64 %t_2329, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %cond_2331 = icmp eq i64 %fn_ret_2330, 10
  br label %sc_and_done_2320

sc_and_done_2320:
  br label %sc_and_merge_2320

sc_and_merge_2320:
  %and_cond_2332 = phi i1 [ false, %sc_and_short_2320 ], [ %cond_2331, %sc_and_done_2320 ]
  br i1 %and_cond_2332, label %sc_and_rhs_2319, label %sc_and_short_2319

sc_and_short_2319:
  br label %sc_and_merge_2319

sc_and_rhs_2319:
  %t64_2333 = load i64, i64* %param_buf_2310
  %t64_2334 = load i64, i64* %var_i_2313
  %t_2335 = add i64 %t64_2333, %t64_2334
  %t_2336 = add i64 %t_2335, 2
  %fn_ret_2337 = call i64 @__omni_cyw_priv_r8(i64 %t_2336, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %cond_2338 = icmp eq i64 %fn_ret_2337, 13
  br label %sc_and_done_2319

sc_and_done_2319:
  br label %sc_and_merge_2319

sc_and_merge_2319:
  %and_cond_2339 = phi i1 [ false, %sc_and_short_2319 ], [ %cond_2338, %sc_and_done_2319 ]
  br i1 %and_cond_2339, label %sc_and_rhs_2318, label %sc_and_short_2318

sc_and_short_2318:
  br label %sc_and_merge_2318

sc_and_rhs_2318:
  %t64_2340 = load i64, i64* %param_buf_2310
  %t64_2341 = load i64, i64* %var_i_2313
  %t_2342 = add i64 %t64_2340, %t64_2341
  %t_2343 = add i64 %t_2342, 3
  %fn_ret_2344 = call i64 @__omni_cyw_priv_r8(i64 %t_2343, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %cond_2345 = icmp eq i64 %fn_ret_2344, 10
  br label %sc_and_done_2318

sc_and_done_2318:
  br label %sc_and_merge_2318

sc_and_merge_2318:
  %and_cond_2346 = phi i1 [ false, %sc_and_short_2318 ], [ %cond_2345, %sc_and_done_2318 ]
  br i1 %and_cond_2346, label %if_then_82, label %if_merge_82

if_then_82:
  %t64_2347 = load i64, i64* %var_i_2313
  %t_2348 = add i64 %t64_2347, 4
  ret i64 %t_2348
if_merge_82:
  call void @__omni_c_return_ptr(i8* null)
  %t64_2349 = load i64, i64* %var_i_2313
  %t_2350 = add i64 %t64_2349, 1
  store i64 %t_2350, i64* %var_i_2313
  br label %while_cond_81

while_exit_81:
  ret i64 0
}


define void @__omni_trampoline_http_find_header_end(i8* %raw_arg) {
entry:
  %raw_int = ptrtoint i8* %raw_arg to i64
  %res = call i64 @__omni_cyw_priv_http_find_header_end(i64 %raw_int, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  ret void
}

define void @__omni_trampoline_pack_http_find_header_end(i8* %raw_pack) {
entry:
  %p64 = bitcast i8* %raw_pack to i64*
  %p0 = getelementptr inbounds i64, i64* %p64, i64 0
  %a0 = load i64, i64* %p0
  %p1 = getelementptr inbounds i64, i64* %p64, i64 1
  %a1 = load i64, i64* %p1
  %p2 = getelementptr inbounds i64, i64* %p64, i64 2
  %a2 = load i64, i64* %p2
  %p3 = getelementptr inbounds i64, i64* %p64, i64 3
  %a3 = load i64, i64* %p3
  %p4 = getelementptr inbounds i64, i64* %p64, i64 4
  %a4 = load i64, i64* %p4
  %p5 = getelementptr inbounds i64, i64* %p64, i64 5
  %a5 = load i64, i64* %p5
  call void @free(i8* %raw_pack)
  %res = call i64 @__omni_cyw_priv_http_find_header_end(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5)
  ret void
}

define i64 @__omni_cyw_priv_http_extract_ws_key(i64 %arg0, i64 %arg1, i64 %arg2, i64 %arg3, i64 %arg4, i64 %arg5) {
entry:
  %param_ctx_2352 = alloca i64
  %param_buf_2354 = alloca i64
  %param_len_2356 = alloca i64
  %param_out_key_2358 = alloca i64
  %var_pat_2362 = alloca i64
  %var_plen_2367 = alloca i64
  %var_i_2368 = alloca i64
  %var_m_2374 = alloca i64
  %var_k_2375 = alloca i64
  %var_p_2398 = alloca i64
  %var_kl_2418 = alloca i64
  %var_c_2434 = alloca i64
  %param_unset_ctx_2351 = icmp eq i64 %arg0, -581005069656919554
  %param_dsel_ctx_2351 = select i1 %param_unset_ctx_2351, i64 0, i64 %arg0
  store i64 %param_dsel_ctx_2351, i64* %param_ctx_2352
  %param_unset_buf_2353 = icmp eq i64 %arg1, -581005069656919554
  %param_dsel_buf_2353 = select i1 %param_unset_buf_2353, i64 0, i64 %arg1
  store i64 %param_dsel_buf_2353, i64* %param_buf_2354
  %param_unset_len_2355 = icmp eq i64 %arg2, -581005069656919554
  %param_dsel_len_2355 = select i1 %param_unset_len_2355, i64 0, i64 %arg2
  store i64 %param_dsel_len_2355, i64* %param_len_2356
  %param_unset_out_key_2357 = icmp eq i64 %arg3, -581005069656919554
  %param_dsel_out_key_2357 = select i1 %param_unset_out_key_2357, i64 0, i64 %arg3
  store i64 %param_dsel_out_key_2357, i64* %param_out_key_2358
  call void @__omni_c_return_ptr(i8* null)
  %t64_2359 = load i64, i64* %param_ctx_2352
  %t_2360 = add i64 %t64_2359, 8000
  %t_2361 = add i64 %t_2360, 128
  store i64 %t_2361, i64* %var_pat_2362
  call void @__omni_c_return_ptr(i8* null)
  %t64_2363 = load i64, i64* %var_pat_2362
  %str_lit_2364 = getelementptr inbounds [19 x i8], [19 x i8]* @.str.cyw_8, i64 0, i64 0
  call void @__omni_c_return_ptr(i8* %str_lit_2364)
  %p2i64_2365 = ptrtoint i8* %str_lit_2364 to i64
  %fn_ret_2366 = call i64 @__omni_cyw_priv_put_ascii(i64 %t64_2363, i64 %p2i64_2365, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  store i64 %fn_ret_2366, i64* %var_plen_2367
  call void @__omni_c_return_ptr(i8* null)
  store i64 0, i64* %var_i_2368
  br label %while_cond_83

while_cond_83:
  %t64_2369 = load i64, i64* %var_i_2368
  %t64_2370 = load i64, i64* %var_plen_2367
  %t_2371 = add i64 %t64_2369, %t64_2370
  %t64_2372 = load i64, i64* %param_len_2356
  %cond_2373 = icmp slt i64 %t_2371, %t64_2372
  br i1 %cond_2373, label %while_body_83, label %while_exit_83

while_body_83:
  call void @__omni_c_return_ptr(i8* null)
  store i64 1, i64* %var_m_2374
  call void @__omni_c_return_ptr(i8* null)
  store i64 0, i64* %var_k_2375
  br label %while_cond_84

while_cond_84:
  %t64_2376 = load i64, i64* %var_k_2375
  %t64_2377 = load i64, i64* %var_plen_2367
  %cond_2378 = icmp slt i64 %t64_2376, %t64_2377
  br i1 %cond_2378, label %while_body_84, label %while_exit_84

while_body_84:
  %t64_2379 = load i64, i64* %param_buf_2354
  %t64_2380 = load i64, i64* %var_i_2368
  %t_2381 = add i64 %t64_2379, %t64_2380
  %t64_2382 = load i64, i64* %var_k_2375
  %t_2383 = add i64 %t_2381, %t64_2382
  %fn_ret_2384 = call i64 @__omni_cyw_priv_r8(i64 %t_2383, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %fn_ret_2385 = call i64 @__omni_cyw_priv_ascii_lower(i64 %fn_ret_2384, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_2386 = load i64, i64* %var_pat_2362
  %t64_2387 = load i64, i64* %var_k_2375
  %t_2388 = add i64 %t64_2386, %t64_2387
  %fn_ret_2389 = call i64 @__omni_cyw_priv_r8(i64 %t_2388, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %cond_2390 = icmp ne i64 %fn_ret_2385, %fn_ret_2389
  br i1 %cond_2390, label %if_then_85, label %if_merge_85

if_then_85:
  call void @__omni_c_return_ptr(i8* null)
  store i64 0, i64* %var_m_2374
  br label %while_exit_84
if_merge_85:
  call void @__omni_c_return_ptr(i8* null)
  %t64_2391 = load i64, i64* %var_k_2375
  %t_2392 = add i64 %t64_2391, 1
  store i64 %t_2392, i64* %var_k_2375
  br label %while_cond_84

while_exit_84:
  %t64_2393 = load i64, i64* %var_m_2374
  %cond_2394 = icmp ne i64 %t64_2393, 0
  br i1 %cond_2394, label %if_then_86, label %if_merge_86

if_then_86:
  call void @__omni_c_return_ptr(i8* null)
  %t64_2395 = load i64, i64* %var_i_2368
  %t64_2396 = load i64, i64* %var_plen_2367
  %t_2397 = add i64 %t64_2395, %t64_2396
  store i64 %t_2397, i64* %var_p_2398
  br label %while_cond_87

while_cond_87:
  %t64_2400 = load i64, i64* %var_p_2398
  %t64_2401 = load i64, i64* %param_len_2356
  %cond_2402 = icmp slt i64 %t64_2400, %t64_2401
  br i1 %cond_2402, label %sc_and_rhs_2399, label %sc_and_short_2399

sc_and_short_2399:
  br label %sc_and_merge_2399

sc_and_rhs_2399:
  %t64_2404 = load i64, i64* %param_buf_2354
  %t64_2405 = load i64, i64* %var_p_2398
  %t_2406 = add i64 %t64_2404, %t64_2405
  %fn_ret_2407 = call i64 @__omni_cyw_priv_r8(i64 %t_2406, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %cond_2408 = icmp eq i64 %fn_ret_2407, 32
  br i1 %cond_2408, label %sc_or_short_2403, label %sc_or_rhs_2403

sc_or_short_2403:
  br label %sc_or_merge_2403

sc_or_rhs_2403:
  %t64_2409 = load i64, i64* %param_buf_2354
  %t64_2410 = load i64, i64* %var_p_2398
  %t_2411 = add i64 %t64_2409, %t64_2410
  %fn_ret_2412 = call i64 @__omni_cyw_priv_r8(i64 %t_2411, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %cond_2413 = icmp eq i64 %fn_ret_2412, 9
  br label %sc_or_done_2403

sc_or_done_2403:
  br label %sc_or_merge_2403

sc_or_merge_2403:
  %or_cond_2414 = phi i1 [ true, %sc_or_short_2403 ], [ %cond_2413, %sc_or_done_2403 ]
  br label %sc_and_done_2399

sc_and_done_2399:
  br label %sc_and_merge_2399

sc_and_merge_2399:
  %and_cond_2415 = phi i1 [ false, %sc_and_short_2399 ], [ %or_cond_2414, %sc_and_done_2399 ]
  br i1 %and_cond_2415, label %while_body_87, label %while_exit_87

while_body_87:
  call void @__omni_c_return_ptr(i8* null)
  %t64_2416 = load i64, i64* %var_p_2398
  %t_2417 = add i64 %t64_2416, 1
  store i64 %t_2417, i64* %var_p_2398
  br label %while_cond_87

while_exit_87:
  call void @__omni_c_return_ptr(i8* null)
  store i64 0, i64* %var_kl_2418
  br label %while_cond_88

while_cond_88:
  %t64_2420 = load i64, i64* %var_p_2398
  %t64_2421 = load i64, i64* %var_kl_2418
  %t_2422 = add i64 %t64_2420, %t64_2421
  %t64_2423 = load i64, i64* %param_len_2356
  %cond_2424 = icmp slt i64 %t_2422, %t64_2423
  br i1 %cond_2424, label %sc_and_rhs_2419, label %sc_and_short_2419

sc_and_short_2419:
  br label %sc_and_merge_2419

sc_and_rhs_2419:
  %t64_2425 = load i64, i64* %var_kl_2418
  %cond_2426 = icmp slt i64 %t64_2425, 60
  br label %sc_and_done_2419

sc_and_done_2419:
  br label %sc_and_merge_2419

sc_and_merge_2419:
  %and_cond_2427 = phi i1 [ false, %sc_and_short_2419 ], [ %cond_2426, %sc_and_done_2419 ]
  br i1 %and_cond_2427, label %while_body_88, label %while_exit_88

while_body_88:
  call void @__omni_c_return_ptr(i8* null)
  %t64_2428 = load i64, i64* %param_buf_2354
  %t64_2429 = load i64, i64* %var_p_2398
  %t_2430 = add i64 %t64_2428, %t64_2429
  %t64_2431 = load i64, i64* %var_kl_2418
  %t_2432 = add i64 %t_2430, %t64_2431
  %fn_ret_2433 = call i64 @__omni_cyw_priv_r8(i64 %t_2432, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  store i64 %fn_ret_2433, i64* %var_c_2434
  %t64_2438 = load i64, i64* %var_c_2434
  %cond_2439 = icmp eq i64 %t64_2438, 13
  br i1 %cond_2439, label %sc_or_short_2437, label %sc_or_rhs_2437

sc_or_short_2437:
  br label %sc_or_merge_2437

sc_or_rhs_2437:
  %t64_2440 = load i64, i64* %var_c_2434
  %cond_2441 = icmp eq i64 %t64_2440, 10
  br label %sc_or_done_2437

sc_or_done_2437:
  br label %sc_or_merge_2437

sc_or_merge_2437:
  %or_cond_2442 = phi i1 [ true, %sc_or_short_2437 ], [ %cond_2441, %sc_or_done_2437 ]
  br i1 %or_cond_2442, label %sc_or_short_2436, label %sc_or_rhs_2436

sc_or_short_2436:
  br label %sc_or_merge_2436

sc_or_rhs_2436:
  %t64_2443 = load i64, i64* %var_c_2434
  %cond_2444 = icmp eq i64 %t64_2443, 32
  br label %sc_or_done_2436

sc_or_done_2436:
  br label %sc_or_merge_2436

sc_or_merge_2436:
  %or_cond_2445 = phi i1 [ true, %sc_or_short_2436 ], [ %cond_2444, %sc_or_done_2436 ]
  br i1 %or_cond_2445, label %sc_or_short_2435, label %sc_or_rhs_2435

sc_or_short_2435:
  br label %sc_or_merge_2435

sc_or_rhs_2435:
  %t64_2446 = load i64, i64* %var_c_2434
  %cond_2447 = icmp eq i64 %t64_2446, 9
  br label %sc_or_done_2435

sc_or_done_2435:
  br label %sc_or_merge_2435

sc_or_merge_2435:
  %or_cond_2448 = phi i1 [ true, %sc_or_short_2435 ], [ %cond_2447, %sc_or_done_2435 ]
  br i1 %or_cond_2448, label %if_then_89, label %if_merge_89

if_then_89:
  br label %while_exit_88
if_merge_89:
  %t64_2449 = load i64, i64* %param_out_key_2358
  %t64_2450 = load i64, i64* %var_kl_2418
  %t_2451 = add i64 %t64_2449, %t64_2450
  %t64_2452 = load i64, i64* %var_c_2434
  %fn_ret_2453 = call i64 @__omni_cyw_priv_w8(i64 %t_2451, i64 %t64_2452, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  call void @__omni_c_return_ptr(i8* null)
  %t64_2454 = load i64, i64* %var_kl_2418
  %t_2455 = add i64 %t64_2454, 1
  store i64 %t_2455, i64* %var_kl_2418
  br label %while_cond_88

while_exit_88:
  %t64_2456 = load i64, i64* %var_kl_2418
  ret i64 %t64_2456
if_merge_86:
  call void @__omni_c_return_ptr(i8* null)
  %t64_2457 = load i64, i64* %var_i_2368
  %t_2458 = add i64 %t64_2457, 1
  store i64 %t_2458, i64* %var_i_2368
  br label %while_cond_83

while_exit_83:
  ret i64 0
}


define void @__omni_trampoline_http_extract_ws_key(i8* %raw_arg) {
entry:
  %raw_int = ptrtoint i8* %raw_arg to i64
  %res = call i64 @__omni_cyw_priv_http_extract_ws_key(i64 %raw_int, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  ret void
}

define void @__omni_trampoline_pack_http_extract_ws_key(i8* %raw_pack) {
entry:
  %p64 = bitcast i8* %raw_pack to i64*
  %p0 = getelementptr inbounds i64, i64* %p64, i64 0
  %a0 = load i64, i64* %p0
  %p1 = getelementptr inbounds i64, i64* %p64, i64 1
  %a1 = load i64, i64* %p1
  %p2 = getelementptr inbounds i64, i64* %p64, i64 2
  %a2 = load i64, i64* %p2
  %p3 = getelementptr inbounds i64, i64* %p64, i64 3
  %a3 = load i64, i64* %p3
  %p4 = getelementptr inbounds i64, i64* %p64, i64 4
  %a4 = load i64, i64* %p4
  %p5 = getelementptr inbounds i64, i64* %p64, i64 5
  %a5 = load i64, i64* %p5
  call void @free(i8* %raw_pack)
  %res = call i64 @__omni_cyw_priv_http_extract_ws_key(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5)
  ret void
}

define i64 @__omni_cyw_priv_ws_handle_frames(i64 %arg0, i64 %arg1, i64 %arg2, i64 %arg3, i64 %arg4, i64 %arg5) {
entry:
  %param_ctx_2460 = alloca i64
  %param_buf_2462 = alloca i64
  %param_len_2464 = alloca i64
  %var_pos_2465 = alloca i64
  %var_b0_2474 = alloca i64
  %var_b1_2480 = alloca i64
  %var_opcode_2483 = alloca i64
  %var_masked_2487 = alloca i64
  %var_plen_2490 = alloca i64
  %var_hdr_2491 = alloca i64
  %var_moff_2508 = alloca i64
  %var_dst_2522 = alloca i64
  %var_copy_n_2524 = alloca i64
  %var_i_2527 = alloca i64
  %var_v_2539 = alloca i64
  %param_unset_ctx_2459 = icmp eq i64 %arg0, -581005069656919554
  %param_dsel_ctx_2459 = select i1 %param_unset_ctx_2459, i64 0, i64 %arg0
  store i64 %param_dsel_ctx_2459, i64* %param_ctx_2460
  %param_unset_buf_2461 = icmp eq i64 %arg1, -581005069656919554
  %param_dsel_buf_2461 = select i1 %param_unset_buf_2461, i64 0, i64 %arg1
  store i64 %param_dsel_buf_2461, i64* %param_buf_2462
  %param_unset_len_2463 = icmp eq i64 %arg2, -581005069656919554
  %param_dsel_len_2463 = select i1 %param_unset_len_2463, i64 0, i64 %arg2
  store i64 %param_dsel_len_2463, i64* %param_len_2464
  call void @__omni_c_return_ptr(i8* null)
  store i64 0, i64* %var_pos_2465
  br label %while_cond_90

while_cond_90:
  %t64_2466 = load i64, i64* %var_pos_2465
  %t_2467 = add i64 %t64_2466, 2
  %t64_2468 = load i64, i64* %param_len_2464
  %cond_2469 = icmp sle i64 %t_2467, %t64_2468
  br i1 %cond_2469, label %while_body_90, label %while_exit_90

while_body_90:
  call void @__omni_c_return_ptr(i8* null)
  %t64_2470 = load i64, i64* %param_buf_2462
  %t64_2471 = load i64, i64* %var_pos_2465
  %t_2472 = add i64 %t64_2470, %t64_2471
  %fn_ret_2473 = call i64 @__omni_cyw_priv_r8(i64 %t_2472, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  store i64 %fn_ret_2473, i64* %var_b0_2474
  call void @__omni_c_return_ptr(i8* null)
  %t64_2475 = load i64, i64* %param_buf_2462
  %t64_2476 = load i64, i64* %var_pos_2465
  %t_2477 = add i64 %t64_2475, %t64_2476
  %t_2478 = add i64 %t_2477, 1
  %fn_ret_2479 = call i64 @__omni_cyw_priv_r8(i64 %t_2478, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  store i64 %fn_ret_2479, i64* %var_b1_2480
  call void @__omni_c_return_ptr(i8* null)
  %t64_2481 = load i64, i64* %var_b0_2474
  %t_2482 = and i64 %t64_2481, 15
  store i64 %t_2482, i64* %var_opcode_2483
  call void @__omni_c_return_ptr(i8* null)
  %t64_2484 = load i64, i64* %var_b1_2480
  %t_2485 = ashr i64 %t64_2484, 7
  %t_2486 = and i64 %t_2485, 1
  store i64 %t_2486, i64* %var_masked_2487
  call void @__omni_c_return_ptr(i8* null)
  %t64_2488 = load i64, i64* %var_b1_2480
  %t_2489 = and i64 %t64_2488, 127
  store i64 %t_2489, i64* %var_plen_2490
  call void @__omni_c_return_ptr(i8* null)
  store i64 2, i64* %var_hdr_2491
  %t64_2492 = load i64, i64* %var_plen_2490
  %cond_2493 = icmp eq i64 %t64_2492, 126
  br i1 %cond_2493, label %if_then_91, label %if_else_91

if_then_91:
  %t64_2494 = load i64, i64* %var_pos_2465
  %t_2495 = add i64 %t64_2494, 4
  %t64_2496 = load i64, i64* %param_len_2464
  %cond_2497 = icmp sgt i64 %t_2495, %t64_2496
  br i1 %cond_2497, label %if_then_92, label %if_merge_92

if_then_92:
  br label %while_exit_90
if_merge_92:
  call void @__omni_c_return_ptr(i8* null)
  %t64_2498 = load i64, i64* %param_buf_2462
  %t64_2499 = load i64, i64* %var_pos_2465
  %t_2500 = add i64 %t64_2498, %t64_2499
  %t_2501 = add i64 %t_2500, 2
  %fn_ret_2502 = call i64 @__omni_cyw_priv_r16_be(i64 %t_2501, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  store i64 %fn_ret_2502, i64* %var_plen_2490
  call void @__omni_c_return_ptr(i8* null)
  store i64 4, i64* %var_hdr_2491
  br label %if_merge_91

if_else_91:
  %t64_2503 = load i64, i64* %var_plen_2490
  %cond_2504 = icmp eq i64 %t64_2503, 127
  br i1 %cond_2504, label %if_then_93, label %if_merge_93

if_then_93:
  br label %while_exit_90
if_merge_93:
  br label %if_merge_91

if_merge_91:
  call void @__omni_c_return_ptr(i8* null)
  %t64_2505 = load i64, i64* %var_pos_2465
  %t64_2506 = load i64, i64* %var_hdr_2491
  %t_2507 = add i64 %t64_2505, %t64_2506
  store i64 %t_2507, i64* %var_moff_2508
  %t64_2509 = load i64, i64* %var_masked_2487
  %cond_2510 = icmp ne i64 %t64_2509, 0
  br i1 %cond_2510, label %if_then_94, label %if_merge_94

if_then_94:
  call void @__omni_c_return_ptr(i8* null)
  %t64_2511 = load i64, i64* %var_hdr_2491
  %t_2512 = add i64 %t64_2511, 4
  store i64 %t_2512, i64* %var_hdr_2491
  br label %if_merge_94

if_merge_94:
  %t64_2513 = load i64, i64* %var_pos_2465
  %t64_2514 = load i64, i64* %var_hdr_2491
  %t_2515 = add i64 %t64_2513, %t64_2514
  %t64_2516 = load i64, i64* %var_plen_2490
  %t_2517 = add i64 %t_2515, %t64_2516
  %t64_2518 = load i64, i64* %param_len_2464
  %cond_2519 = icmp sgt i64 %t_2517, %t64_2518
  br i1 %cond_2519, label %if_then_95, label %if_merge_95

if_then_95:
  br label %while_exit_90
if_merge_95:
  call void @__omni_c_return_ptr(i8* null)
  %t64_2520 = load i64, i64* %param_ctx_2460
  %t_2521 = add i64 %t64_2520, 1184
  store i64 %t_2521, i64* %var_dst_2522
  call void @__omni_c_return_ptr(i8* null)
  %t64_2523 = load i64, i64* %var_plen_2490
  store i64 %t64_2523, i64* %var_copy_n_2524
  %t64_2525 = load i64, i64* %var_copy_n_2524
  %cond_2526 = icmp sgt i64 %t64_2525, 1024
  br i1 %cond_2526, label %if_then_96, label %if_merge_96

if_then_96:
  call void @__omni_c_return_ptr(i8* null)
  store i64 1024, i64* %var_copy_n_2524
  br label %if_merge_96

if_merge_96:
  call void @__omni_c_return_ptr(i8* null)
  store i64 0, i64* %var_i_2527
  br label %while_cond_97

while_cond_97:
  %t64_2528 = load i64, i64* %var_i_2527
  %t64_2529 = load i64, i64* %var_copy_n_2524
  %cond_2530 = icmp slt i64 %t64_2528, %t64_2529
  br i1 %cond_2530, label %while_body_97, label %while_exit_97

while_body_97:
  call void @__omni_c_return_ptr(i8* null)
  %t64_2531 = load i64, i64* %param_buf_2462
  %t64_2532 = load i64, i64* %var_pos_2465
  %t_2533 = add i64 %t64_2531, %t64_2532
  %t64_2534 = load i64, i64* %var_hdr_2491
  %t_2535 = add i64 %t_2533, %t64_2534
  %t64_2536 = load i64, i64* %var_i_2527
  %t_2537 = add i64 %t_2535, %t64_2536
  %fn_ret_2538 = call i64 @__omni_cyw_priv_r8(i64 %t_2537, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  store i64 %fn_ret_2538, i64* %var_v_2539
  %t64_2540 = load i64, i64* %var_masked_2487
  %cond_2541 = icmp ne i64 %t64_2540, 0
  br i1 %cond_2541, label %if_then_98, label %if_merge_98

if_then_98:
  call void @__omni_c_return_ptr(i8* null)
  %t64_2542 = load i64, i64* %var_v_2539
  %t64_2543 = load i64, i64* %param_buf_2462
  %t64_2544 = load i64, i64* %var_moff_2508
  %t_2545 = add i64 %t64_2543, %t64_2544
  %t64_2546 = load i64, i64* %var_i_2527
  %t_2547 = and i64 %t64_2546, 3
  %t_2548 = add i64 %t_2545, %t_2547
  %fn_ret_2549 = call i64 @__omni_cyw_priv_r8(i64 %t_2548, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t_2550 = xor i64 %t64_2542, %fn_ret_2549
  store i64 %t_2550, i64* %var_v_2539
  br label %if_merge_98

if_merge_98:
  %t64_2551 = load i64, i64* %var_dst_2522
  %t64_2552 = load i64, i64* %var_i_2527
  %t_2553 = add i64 %t64_2551, %t64_2552
  %t64_2554 = load i64, i64* %var_v_2539
  %fn_ret_2555 = call i64 @__omni_cyw_priv_w8(i64 %t_2553, i64 %t64_2554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  call void @__omni_c_return_ptr(i8* null)
  %t64_2556 = load i64, i64* %var_i_2527
  %t_2557 = add i64 %t64_2556, 1
  store i64 %t_2557, i64* %var_i_2527
  br label %while_cond_97

while_exit_97:
  %t64_2558 = load i64, i64* %var_dst_2522
  %t64_2559 = load i64, i64* %var_copy_n_2524
  %t_2560 = add i64 %t64_2558, %t64_2559
  %fn_ret_2561 = call i64 @__omni_cyw_priv_w8(i64 %t_2560, i64 0, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_2562 = load i64, i64* %var_opcode_2483
  %cond_2563 = icmp eq i64 %t64_2562, 9
  br i1 %cond_2563, label %if_then_99, label %if_else_99

if_then_99:
  %t64_2564 = load i64, i64* %param_ctx_2460
  %t64_2565 = load i64, i64* %var_dst_2522
  %t64_2566 = load i64, i64* %var_copy_n_2524
  %fn_ret_2567 = call i64 @__omni_cyw_priv_ws_send_raw_frame(i64 %t64_2564, i64 10, i64 %t64_2565, i64 %t64_2566, i64 -581005069656919554, i64 -581005069656919554)
  br label %if_merge_99

if_else_99:
  %t64_2568 = load i64, i64* %var_opcode_2483
  %cond_2569 = icmp eq i64 %t64_2568, 8
  br i1 %cond_2569, label %if_then_100, label %if_else_100

if_then_100:
  %t64_2570 = load i64, i64* %param_ctx_2460
  %fn_ret_2571 = call i64 @__omni_cyw_priv_ws_send_raw_frame(i64 %t64_2570, i64 8, i64 0, i64 0, i64 -581005069656919554, i64 -581005069656919554)
  %t64_2572 = load i64, i64* %param_ctx_2460
  %fn_ret_2573 = call i64 @__omni_cyw_priv_tcp_send_tracked(i64 %t64_2572, i64 17, i64 0, i64 0, i64 -581005069656919554, i64 -581005069656919554)
  %t64_2574 = load i64, i64* %param_ctx_2460
  %t_2575 = add i64 %t64_2574, 12
  %ext_i64_2576 = call i64 @mmio_write32(i64 %t_2575, i64 1)
  %t64_2577 = load i64, i64* %param_ctx_2460
  %t_2578 = add i64 %t64_2577, 100
  %ext_i64_2579 = call i64 @mmio_write32(i64 %t_2578, i64 0)
  ret i64 0
if_else_100:
  %t64_2582 = load i64, i64* %var_opcode_2483
  %cond_2583 = icmp eq i64 %t64_2582, 1
  br i1 %cond_2583, label %sc_or_short_2581, label %sc_or_rhs_2581

sc_or_short_2581:
  br label %sc_or_merge_2581

sc_or_rhs_2581:
  %t64_2584 = load i64, i64* %var_opcode_2483
  %cond_2585 = icmp eq i64 %t64_2584, 2
  br label %sc_or_done_2581

sc_or_done_2581:
  br label %sc_or_merge_2581

sc_or_merge_2581:
  %or_cond_2586 = phi i1 [ true, %sc_or_short_2581 ], [ %cond_2585, %sc_or_done_2581 ]
  br i1 %or_cond_2586, label %sc_or_short_2580, label %sc_or_rhs_2580

sc_or_short_2580:
  br label %sc_or_merge_2580

sc_or_rhs_2580:
  %t64_2587 = load i64, i64* %var_opcode_2483
  %cond_2588 = icmp eq i64 %t64_2587, 0
  br label %sc_or_done_2580

sc_or_done_2580:
  br label %sc_or_merge_2580

sc_or_merge_2580:
  %or_cond_2589 = phi i1 [ true, %sc_or_short_2580 ], [ %cond_2588, %sc_or_done_2580 ]
  br i1 %or_cond_2589, label %if_then_101, label %if_merge_101

if_then_101:
  %t64_2590 = load i64, i64* %param_ctx_2460
  %t_2591 = add i64 %t64_2590, 120
  %ext_i64_2592 = call i64 @mmio_write32(i64 %t_2591, i64 1)
  br label %if_merge_101

if_merge_101:
  br label %if_merge_100

if_merge_100:
  br label %if_merge_99

if_merge_99:
  call void @__omni_c_return_ptr(i8* null)
  %t64_2593 = load i64, i64* %var_pos_2465
  %t64_2594 = load i64, i64* %var_hdr_2491
  %t_2595 = add i64 %t64_2593, %t64_2594
  %t64_2596 = load i64, i64* %var_plen_2490
  %t_2597 = add i64 %t_2595, %t64_2596
  store i64 %t_2597, i64* %var_pos_2465
  br label %while_cond_90

while_exit_90:
  ret i64 0
}


define void @__omni_trampoline_ws_handle_frames(i8* %raw_arg) {
entry:
  %raw_int = ptrtoint i8* %raw_arg to i64
  %res = call i64 @__omni_cyw_priv_ws_handle_frames(i64 %raw_int, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  ret void
}

define void @__omni_trampoline_pack_ws_handle_frames(i8* %raw_pack) {
entry:
  %p64 = bitcast i8* %raw_pack to i64*
  %p0 = getelementptr inbounds i64, i64* %p64, i64 0
  %a0 = load i64, i64* %p0
  %p1 = getelementptr inbounds i64, i64* %p64, i64 1
  %a1 = load i64, i64* %p1
  %p2 = getelementptr inbounds i64, i64* %p64, i64 2
  %a2 = load i64, i64* %p2
  %p3 = getelementptr inbounds i64, i64* %p64, i64 3
  %a3 = load i64, i64* %p3
  %p4 = getelementptr inbounds i64, i64* %p64, i64 4
  %a4 = load i64, i64* %p4
  %p5 = getelementptr inbounds i64, i64* %p64, i64 5
  %a5 = load i64, i64* %p5
  call void @free(i8* %raw_pack)
  %res = call i64 @__omni_cyw_priv_ws_handle_frames(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5)
  ret void
}

define i64 @__omni_cyw_priv_tcp_ws_rx(i64 %arg0, i64 %arg1, i64 %arg2, i64 %arg3, i64 %arg4, i64 %arg5) {
entry:
  %param_ctx_2599 = alloca i64
  %param_eth_2601 = alloca i64
  %param_ip_2603 = alloca i64
  %param_ihl_2605 = alloca i64
  %param_ip_tot_len_2607 = alloca i64
  %var_tcp_2611 = alloca i64
  %var_tcp_seg_len_2615 = alloca i64
  %var_sport_2621 = alloca i64
  %var_dport_2625 = alloca i64
  %var_ws_port_2629 = alloca i64
  %var_rx_seq_2636 = alloca i64
  %var_rx_ack_2640 = alloca i64
  %var_thlen_2647 = alloca i64
  %var_flags_2658 = alloca i64
  %var_data_ptr_2662 = alloca i64
  %var_data_len_2666 = alloca i64
  %var_state_2670 = alloca i64
  %var_isn_2728 = alloca i64
  %var_exp_ack_2814 = alloca i64
  %var_hlen_2832 = alloca i64
  %var_hbuf_2835 = alloca i64
  %var_room_2838 = alloca i64
  %var_cp_2840 = alloca i64
  %var_hend_2863 = alloca i64
  %var_key_tmp_2871 = alloca i64
  %var_acc_tmp_2875 = alloca i64
  %var_klen_2881 = alloca i64
  %var_resp_2891 = alloca i64
  %var_rp_2892 = alloca i64
  %var_cp_2948 = alloca i64
  %var_cur_ack_2974 = alloca i64
  %param_unset_ctx_2598 = icmp eq i64 %arg0, -581005069656919554
  %param_dsel_ctx_2598 = select i1 %param_unset_ctx_2598, i64 0, i64 %arg0
  store i64 %param_dsel_ctx_2598, i64* %param_ctx_2599
  %param_unset_eth_2600 = icmp eq i64 %arg1, -581005069656919554
  %param_dsel_eth_2600 = select i1 %param_unset_eth_2600, i64 0, i64 %arg1
  store i64 %param_dsel_eth_2600, i64* %param_eth_2601
  %param_unset_ip_2602 = icmp eq i64 %arg2, -581005069656919554
  %param_dsel_ip_2602 = select i1 %param_unset_ip_2602, i64 0, i64 %arg2
  store i64 %param_dsel_ip_2602, i64* %param_ip_2603
  %param_unset_ihl_2604 = icmp eq i64 %arg3, -581005069656919554
  %param_dsel_ihl_2604 = select i1 %param_unset_ihl_2604, i64 0, i64 %arg3
  store i64 %param_dsel_ihl_2604, i64* %param_ihl_2605
  %param_unset_ip_tot_len_2606 = icmp eq i64 %arg4, -581005069656919554
  %param_dsel_ip_tot_len_2606 = select i1 %param_unset_ip_tot_len_2606, i64 0, i64 %arg4
  store i64 %param_dsel_ip_tot_len_2606, i64* %param_ip_tot_len_2607
  call void @__omni_c_return_ptr(i8* null)
  %t64_2608 = load i64, i64* %param_ip_2603
  %t64_2609 = load i64, i64* %param_ihl_2605
  %t_2610 = add i64 %t64_2608, %t64_2609
  store i64 %t_2610, i64* %var_tcp_2611
  call void @__omni_c_return_ptr(i8* null)
  %t64_2612 = load i64, i64* %param_ip_tot_len_2607
  %t64_2613 = load i64, i64* %param_ihl_2605
  %t_2614 = sub i64 %t64_2612, %t64_2613
  store i64 %t_2614, i64* %var_tcp_seg_len_2615
  %t64_2616 = load i64, i64* %var_tcp_seg_len_2615
  %cond_2617 = icmp slt i64 %t64_2616, 20
  br i1 %cond_2617, label %if_then_102, label %if_merge_102

if_then_102:
  ret i64 0
if_merge_102:
  call void @__omni_c_return_ptr(i8* null)
  %t64_2618 = load i64, i64* %var_tcp_2611
  %t_2619 = add i64 %t64_2618, 0
  %fn_ret_2620 = call i64 @__omni_cyw_priv_r16_be(i64 %t_2619, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  store i64 %fn_ret_2620, i64* %var_sport_2621
  call void @__omni_c_return_ptr(i8* null)
  %t64_2622 = load i64, i64* %var_tcp_2611
  %t_2623 = add i64 %t64_2622, 2
  %fn_ret_2624 = call i64 @__omni_cyw_priv_r16_be(i64 %t_2623, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  store i64 %fn_ret_2624, i64* %var_dport_2625
  call void @__omni_c_return_ptr(i8* null)
  %t64_2626 = load i64, i64* %param_ctx_2599
  %t_2627 = add i64 %t64_2626, 16
  %ext_i64_2628 = call i64 @mmio_read32(i64 %t_2627)
  store i64 %ext_i64_2628, i64* %var_ws_port_2629
  %t64_2630 = load i64, i64* %var_dport_2625
  %t64_2631 = load i64, i64* %var_ws_port_2629
  %cond_2632 = icmp ne i64 %t64_2630, %t64_2631
  br i1 %cond_2632, label %if_then_103, label %if_merge_103

if_then_103:
  ret i64 0
if_merge_103:
  call void @__omni_c_return_ptr(i8* null)
  %t64_2633 = load i64, i64* %var_tcp_2611
  %t_2634 = add i64 %t64_2633, 4
  %fn_ret_2635 = call i64 @__omni_cyw_priv_r32_be(i64 %t_2634, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  store i64 %fn_ret_2635, i64* %var_rx_seq_2636
  call void @__omni_c_return_ptr(i8* null)
  %t64_2637 = load i64, i64* %var_tcp_2611
  %t_2638 = add i64 %t64_2637, 8
  %fn_ret_2639 = call i64 @__omni_cyw_priv_r32_be(i64 %t_2638, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  store i64 %fn_ret_2639, i64* %var_rx_ack_2640
  call void @__omni_c_return_ptr(i8* null)
  %t64_2641 = load i64, i64* %var_tcp_2611
  %t_2642 = add i64 %t64_2641, 12
  %fn_ret_2643 = call i64 @__omni_cyw_priv_r8(i64 %t_2642, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t_2644 = ashr i64 %fn_ret_2643, 4
  %t_2645 = and i64 %t_2644, 15
  %t_2646 = mul i64 %t_2645, 4
  store i64 %t_2646, i64* %var_thlen_2647
  %t64_2649 = load i64, i64* %var_thlen_2647
  %cond_2650 = icmp slt i64 %t64_2649, 20
  br i1 %cond_2650, label %sc_or_short_2648, label %sc_or_rhs_2648

sc_or_short_2648:
  br label %sc_or_merge_2648

sc_or_rhs_2648:
  %t64_2651 = load i64, i64* %var_thlen_2647
  %t64_2652 = load i64, i64* %var_tcp_seg_len_2615
  %cond_2653 = icmp sgt i64 %t64_2651, %t64_2652
  br label %sc_or_done_2648

sc_or_done_2648:
  br label %sc_or_merge_2648

sc_or_merge_2648:
  %or_cond_2654 = phi i1 [ true, %sc_or_short_2648 ], [ %cond_2653, %sc_or_done_2648 ]
  br i1 %or_cond_2654, label %if_then_104, label %if_merge_104

if_then_104:
  ret i64 0
if_merge_104:
  call void @__omni_c_return_ptr(i8* null)
  %t64_2655 = load i64, i64* %var_tcp_2611
  %t_2656 = add i64 %t64_2655, 13
  %fn_ret_2657 = call i64 @__omni_cyw_priv_r8(i64 %t_2656, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  store i64 %fn_ret_2657, i64* %var_flags_2658
  call void @__omni_c_return_ptr(i8* null)
  %t64_2659 = load i64, i64* %var_tcp_2611
  %t64_2660 = load i64, i64* %var_thlen_2647
  %t_2661 = add i64 %t64_2659, %t64_2660
  store i64 %t_2661, i64* %var_data_ptr_2662
  call void @__omni_c_return_ptr(i8* null)
  %t64_2663 = load i64, i64* %var_tcp_seg_len_2615
  %t64_2664 = load i64, i64* %var_thlen_2647
  %t_2665 = sub i64 %t64_2663, %t64_2664
  store i64 %t_2665, i64* %var_data_len_2666
  call void @__omni_c_return_ptr(i8* null)
  %t64_2667 = load i64, i64* %param_ctx_2599
  %t_2668 = add i64 %t64_2667, 12
  %ext_i64_2669 = call i64 @mmio_read32(i64 %t_2668)
  store i64 %ext_i64_2669, i64* %var_state_2670
  %t64_2671 = load i64, i64* %var_state_2670
  %cond_2672 = icmp eq i64 %t64_2671, 0
  br i1 %cond_2672, label %if_then_105, label %if_merge_105

if_then_105:
  ret i64 0
if_merge_105:
  %t64_2673 = load i64, i64* %var_flags_2658
  %t_2674 = and i64 %t64_2673, 4
  %cond_2675 = icmp ne i64 %t_2674, 0
  br i1 %cond_2675, label %if_then_106, label %if_merge_106

if_then_106:
  %t64_2678 = load i64, i64* %var_state_2670
  %cond_2679 = icmp sge i64 %t64_2678, 2
  br i1 %cond_2679, label %sc_and_rhs_2677, label %sc_and_short_2677

sc_and_short_2677:
  br label %sc_and_merge_2677

sc_and_rhs_2677:
  %t64_2680 = load i64, i64* %var_sport_2621
  %t64_2681 = load i64, i64* %param_ctx_2599
  %t_2682 = add i64 %t64_2681, 84
  %ext_i64_2683 = call i64 @mmio_read32(i64 %t_2682)
  %cond_2684 = icmp eq i64 %t64_2680, %ext_i64_2683
  br label %sc_and_done_2677

sc_and_done_2677:
  br label %sc_and_merge_2677

sc_and_merge_2677:
  %and_cond_2685 = phi i1 [ false, %sc_and_short_2677 ], [ %cond_2684, %sc_and_done_2677 ]
  br i1 %and_cond_2685, label %sc_and_rhs_2676, label %sc_and_short_2676

sc_and_short_2676:
  br label %sc_and_merge_2676

sc_and_rhs_2676:
  %t64_2686 = load i64, i64* %param_ip_2603
  %t_2687 = add i64 %t64_2686, 12
  %t64_2688 = load i64, i64* %param_ctx_2599
  %t_2689 = add i64 %t64_2688, 80
  %fn_ret_2690 = call i64 @__omni_cyw_priv_ip_eq(i64 %t_2687, i64 %t_2689, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %cond_2691 = icmp ne i64 %fn_ret_2690, 0
  br label %sc_and_done_2676

sc_and_done_2676:
  br label %sc_and_merge_2676

sc_and_merge_2676:
  %and_cond_2692 = phi i1 [ false, %sc_and_short_2676 ], [ %cond_2691, %sc_and_done_2676 ]
  br i1 %and_cond_2692, label %if_then_107, label %if_merge_107

if_then_107:
  %t64_2693 = load i64, i64* %param_ctx_2599
  %t_2694 = add i64 %t64_2693, 12
  %ext_i64_2695 = call i64 @mmio_write32(i64 %t_2694, i64 1)
  %t64_2696 = load i64, i64* %param_ctx_2599
  %t_2697 = add i64 %t64_2696, 100
  %ext_i64_2698 = call i64 @mmio_write32(i64 %t_2697, i64 0)
  %t64_2699 = load i64, i64* %param_ctx_2599
  %t_2700 = add i64 %t64_2699, 116
  %ext_i64_2701 = call i64 @mmio_write32(i64 %t_2700, i64 0)
  br label %if_merge_107

if_merge_107:
  ret i64 0
if_merge_106:
  %t64_2703 = load i64, i64* %var_flags_2658
  %t_2704 = and i64 %t64_2703, 2
  %cond_2705 = icmp ne i64 %t_2704, 0
  br i1 %cond_2705, label %sc_and_rhs_2702, label %sc_and_short_2702

sc_and_short_2702:
  br label %sc_and_merge_2702

sc_and_rhs_2702:
  %t64_2706 = load i64, i64* %var_flags_2658
  %t_2707 = and i64 %t64_2706, 16
  %cond_2708 = icmp eq i64 %t_2707, 0
  br label %sc_and_done_2702

sc_and_done_2702:
  br label %sc_and_merge_2702

sc_and_merge_2702:
  %and_cond_2709 = phi i1 [ false, %sc_and_short_2702 ], [ %cond_2708, %sc_and_done_2702 ]
  br i1 %and_cond_2709, label %if_then_108, label %if_merge_108

if_then_108:
  %t64_2710 = load i64, i64* %param_ctx_2599
  %t_2711 = add i64 %t64_2710, 72
  %t64_2712 = load i64, i64* %param_eth_2601
  %t_2713 = add i64 %t64_2712, 6
  %fn_ret_2714 = call i64 @__omni_cyw_priv_bcopy(i64 %t_2711, i64 %t_2713, i64 6, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_2715 = load i64, i64* %param_ctx_2599
  %t_2716 = add i64 %t64_2715, 80
  %t64_2717 = load i64, i64* %param_ip_2603
  %t_2718 = add i64 %t64_2717, 12
  %fn_ret_2719 = call i64 @__omni_cyw_priv_bcopy(i64 %t_2716, i64 %t_2718, i64 4, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_2720 = load i64, i64* %param_ctx_2599
  %t_2721 = add i64 %t64_2720, 84
  %t64_2722 = load i64, i64* %var_sport_2621
  %ext_i64_2723 = call i64 @mmio_write32(i64 %t_2721, i64 %t64_2722)
  call void @__omni_c_return_ptr(i8* null)
  %ext_i64_2724 = call i64 @time_us()
  %t_2725 = and i64 %ext_i64_2724, 1073741823
  %t_2726 = add i64 %t_2725, 270544960
  %t_2727 = and i64 %t_2726, 4294967295
  store i64 %t_2727, i64* %var_isn_2728
  %t64_2729 = load i64, i64* %param_ctx_2599
  %t_2730 = add i64 %t64_2729, 88
  %t64_2731 = load i64, i64* %var_isn_2728
  %ext_i64_2732 = call i64 @mmio_write32(i64 %t_2730, i64 %t64_2731)
  %t64_2733 = load i64, i64* %param_ctx_2599
  %t_2734 = add i64 %t64_2733, 92
  %t64_2735 = load i64, i64* %var_rx_seq_2636
  %t_2736 = add i64 %t64_2735, 1
  %t_2737 = and i64 %t_2736, 4294967295
  %ext_i64_2738 = call i64 @mmio_write32(i64 %t_2734, i64 %t_2737)
  %t64_2739 = load i64, i64* %param_ctx_2599
  %t_2740 = add i64 %t64_2739, 116
  %ext_i64_2741 = call i64 @mmio_write32(i64 %t_2740, i64 0)
  %t64_2742 = load i64, i64* %param_ctx_2599
  %t_2743 = add i64 %t64_2742, 120
  %ext_i64_2744 = call i64 @mmio_write32(i64 %t_2743, i64 0)
  %t64_2745 = load i64, i64* %param_ctx_2599
  %t_2746 = add i64 %t64_2745, 12
  %ext_i64_2747 = call i64 @mmio_write32(i64 %t_2746, i64 2)
  %t64_2748 = load i64, i64* %param_ctx_2599
  %fn_ret_2749 = call i64 @__omni_cyw_priv_tcp_send_tracked(i64 %t64_2748, i64 18, i64 0, i64 0, i64 -581005069656919554, i64 -581005069656919554)
  ret i64 1
if_merge_108:
  %t64_2752 = load i64, i64* %var_state_2670
  %cond_2753 = icmp slt i64 %t64_2752, 2
  br i1 %cond_2753, label %sc_or_short_2751, label %sc_or_rhs_2751

sc_or_short_2751:
  br label %sc_or_merge_2751

sc_or_rhs_2751:
  %t64_2754 = load i64, i64* %var_sport_2621
  %t64_2755 = load i64, i64* %param_ctx_2599
  %t_2756 = add i64 %t64_2755, 84
  %ext_i64_2757 = call i64 @mmio_read32(i64 %t_2756)
  %cond_2758 = icmp ne i64 %t64_2754, %ext_i64_2757
  br label %sc_or_done_2751

sc_or_done_2751:
  br label %sc_or_merge_2751

sc_or_merge_2751:
  %or_cond_2759 = phi i1 [ true, %sc_or_short_2751 ], [ %cond_2758, %sc_or_done_2751 ]
  br i1 %or_cond_2759, label %sc_or_short_2750, label %sc_or_rhs_2750

sc_or_short_2750:
  br label %sc_or_merge_2750

sc_or_rhs_2750:
  %t64_2760 = load i64, i64* %param_ip_2603
  %t_2761 = add i64 %t64_2760, 12
  %t64_2762 = load i64, i64* %param_ctx_2599
  %t_2763 = add i64 %t64_2762, 80
  %fn_ret_2764 = call i64 @__omni_cyw_priv_ip_eq(i64 %t_2761, i64 %t_2763, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %cond_2765 = icmp eq i64 %fn_ret_2764, 0
  br label %sc_or_done_2750

sc_or_done_2750:
  br label %sc_or_merge_2750

sc_or_merge_2750:
  %or_cond_2766 = phi i1 [ true, %sc_or_short_2750 ], [ %cond_2765, %sc_or_done_2750 ]
  br i1 %or_cond_2766, label %if_then_109, label %if_merge_109

if_then_109:
  %t64_2767 = load i64, i64* %var_flags_2658
  %t_2768 = and i64 %t64_2767, 4
  %cond_2769 = icmp eq i64 %t_2768, 0
  br i1 %cond_2769, label %if_then_110, label %if_merge_110

if_then_110:
  %t64_2770 = load i64, i64* %param_ctx_2599
  %t64_2771 = load i64, i64* %param_eth_2601
  %t_2772 = add i64 %t64_2771, 6
  %t64_2773 = load i64, i64* %param_ip_2603
  %t_2774 = add i64 %t64_2773, 12
  %t64_2775 = load i64, i64* %var_sport_2621
  %t64_2776 = load i64, i64* %var_ws_port_2629
  %t64_2777 = load i64, i64* %var_rx_ack_2640
  %t64_2778 = load i64, i64* %var_rx_seq_2636
  %t64_2779 = load i64, i64* %var_data_len_2666
  %t_2780 = add i64 %t64_2778, %t64_2779
  %t_2781 = and i64 %t_2780, 4294967295
  %fn_ret_2782 = call i64 @__omni_cyw_priv_tcp_send_segment(i64 %t64_2770, i64 %t_2772, i64 %t_2774, i64 %t64_2775, i64 %t64_2776, i64 %t64_2777, i64 %t_2781, i64 20, i64 0, i64 0)
  br label %if_merge_110

if_merge_110:
  ret i64 0
if_merge_109:
  %t64_2783 = load i64, i64* %var_flags_2658
  %t_2784 = and i64 %t64_2783, 16
  %cond_2785 = icmp ne i64 %t_2784, 0
  br i1 %cond_2785, label %if_then_111, label %if_merge_111

if_then_111:
  %t64_2786 = load i64, i64* %var_rx_ack_2640
  %t64_2787 = load i64, i64* %param_ctx_2599
  %t_2788 = add i64 %t64_2787, 88
  %ext_i64_2789 = call i64 @mmio_read32(i64 %t_2788)
  %cond_2790 = icmp eq i64 %t64_2786, %ext_i64_2789
  br i1 %cond_2790, label %if_then_112, label %if_merge_112

if_then_112:
  %t64_2791 = load i64, i64* %param_ctx_2599
  %t_2792 = add i64 %t64_2791, 100
  %ext_i64_2793 = call i64 @mmio_write32(i64 %t_2792, i64 0)
  %t64_2794 = load i64, i64* %param_ctx_2599
  %t_2795 = add i64 %t64_2794, 112
  %ext_i64_2796 = call i64 @mmio_write32(i64 %t_2795, i64 0)
  %t64_2797 = load i64, i64* %var_state_2670
  %cond_2798 = icmp eq i64 %t64_2797, 2
  br i1 %cond_2798, label %if_then_113, label %if_merge_113

if_then_113:
  %t64_2799 = load i64, i64* %param_ctx_2599
  %t_2800 = add i64 %t64_2799, 124
  %ext_i64_2801 = call i64 @mmio_read32(i64 %t_2800)
  %cond_2802 = icmp ne i64 %ext_i64_2801, 0
  br i1 %cond_2802, label %if_then_114, label %if_else_114

if_then_114:
  call void @__omni_c_return_ptr(i8* null)
  store i64 4, i64* %var_state_2670
  %t64_2803 = load i64, i64* %param_ctx_2599
  %t_2804 = add i64 %t64_2803, 12
  %ext_i64_2805 = call i64 @mmio_write32(i64 %t_2804, i64 4)
  br label %if_merge_114

if_else_114:
  call void @__omni_c_return_ptr(i8* null)
  store i64 3, i64* %var_state_2670
  %t64_2806 = load i64, i64* %param_ctx_2599
  %t_2807 = add i64 %t64_2806, 12
  %ext_i64_2808 = call i64 @mmio_write32(i64 %t_2807, i64 3)
  br label %if_merge_114

if_merge_114:
  br label %if_merge_113

if_merge_113:
  br label %if_merge_112

if_merge_112:
  br label %if_merge_111

if_merge_111:
  %t64_2809 = load i64, i64* %var_data_len_2666
  %cond_2810 = icmp sgt i64 %t64_2809, 0
  br i1 %cond_2810, label %if_then_115, label %if_merge_115

if_then_115:
  call void @__omni_c_return_ptr(i8* null)
  %t64_2811 = load i64, i64* %param_ctx_2599
  %t_2812 = add i64 %t64_2811, 92
  %ext_i64_2813 = call i64 @mmio_read32(i64 %t_2812)
  store i64 %ext_i64_2813, i64* %var_exp_ack_2814
  %t64_2815 = load i64, i64* %var_rx_seq_2636
  %t64_2816 = load i64, i64* %var_exp_ack_2814
  %cond_2817 = icmp ne i64 %t64_2815, %t64_2816
  br i1 %cond_2817, label %if_then_116, label %if_merge_116

if_then_116:
  %t64_2818 = load i64, i64* %param_ctx_2599
  %fn_ret_2819 = call i64 @__omni_cyw_priv_tcp_send_pure_ack(i64 %t64_2818, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  ret i64 1
if_merge_116:
  %t64_2820 = load i64, i64* %param_ctx_2599
  %t_2821 = add i64 %t64_2820, 92
  %t64_2822 = load i64, i64* %var_exp_ack_2814
  %t64_2823 = load i64, i64* %var_data_len_2666
  %t_2824 = add i64 %t64_2822, %t64_2823
  %t_2825 = and i64 %t_2824, 4294967295
  %ext_i64_2826 = call i64 @mmio_write32(i64 %t_2821, i64 %t_2825)
  %t64_2827 = load i64, i64* %var_state_2670
  %cond_2828 = icmp eq i64 %t64_2827, 3
  br i1 %cond_2828, label %if_then_117, label %if_else_117

if_then_117:
  call void @__omni_c_return_ptr(i8* null)
  %t64_2829 = load i64, i64* %param_ctx_2599
  %t_2830 = add i64 %t64_2829, 116
  %ext_i64_2831 = call i64 @mmio_read32(i64 %t_2830)
  store i64 %ext_i64_2831, i64* %var_hlen_2832
  call void @__omni_c_return_ptr(i8* null)
  %t64_2833 = load i64, i64* %param_ctx_2599
  %t_2834 = add i64 %t64_2833, 160
  store i64 %t_2834, i64* %var_hbuf_2835
  call void @__omni_c_return_ptr(i8* null)
  %t64_2836 = load i64, i64* %var_hlen_2832
  %t_2837 = sub i64 1020, %t64_2836
  store i64 %t_2837, i64* %var_room_2838
  call void @__omni_c_return_ptr(i8* null)
  %t64_2839 = load i64, i64* %var_data_len_2666
  store i64 %t64_2839, i64* %var_cp_2840
  %t64_2841 = load i64, i64* %var_cp_2840
  %t64_2842 = load i64, i64* %var_room_2838
  %cond_2843 = icmp sgt i64 %t64_2841, %t64_2842
  br i1 %cond_2843, label %if_then_118, label %if_merge_118

if_then_118:
  call void @__omni_c_return_ptr(i8* null)
  %t64_2844 = load i64, i64* %var_room_2838
  store i64 %t64_2844, i64* %var_cp_2840
  br label %if_merge_118

if_merge_118:
  %t64_2845 = load i64, i64* %var_cp_2840
  %cond_2846 = icmp sgt i64 %t64_2845, 0
  br i1 %cond_2846, label %if_then_119, label %if_merge_119

if_then_119:
  %t64_2847 = load i64, i64* %var_hbuf_2835
  %t64_2848 = load i64, i64* %var_hlen_2832
  %t_2849 = add i64 %t64_2847, %t64_2848
  %t64_2850 = load i64, i64* %var_data_ptr_2662
  %t64_2851 = load i64, i64* %var_cp_2840
  %fn_ret_2852 = call i64 @__omni_cyw_priv_bcopy(i64 %t_2849, i64 %t64_2850, i64 %t64_2851, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  call void @__omni_c_return_ptr(i8* null)
  %t64_2853 = load i64, i64* %var_hlen_2832
  %t64_2854 = load i64, i64* %var_cp_2840
  %t_2855 = add i64 %t64_2853, %t64_2854
  store i64 %t_2855, i64* %var_hlen_2832
  %t64_2856 = load i64, i64* %param_ctx_2599
  %t_2857 = add i64 %t64_2856, 116
  %t64_2858 = load i64, i64* %var_hlen_2832
  %ext_i64_2859 = call i64 @mmio_write32(i64 %t_2857, i64 %t64_2858)
  br label %if_merge_119

if_merge_119:
  call void @__omni_c_return_ptr(i8* null)
  %t64_2860 = load i64, i64* %var_hbuf_2835
  %t64_2861 = load i64, i64* %var_hlen_2832
  %fn_ret_2862 = call i64 @__omni_cyw_priv_http_find_header_end(i64 %t64_2860, i64 %t64_2861, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  store i64 %fn_ret_2862, i64* %var_hend_2863
  %t64_2864 = load i64, i64* %var_hend_2863
  %cond_2865 = icmp eq i64 %t64_2864, 0
  br i1 %cond_2865, label %if_then_120, label %if_else_120

if_then_120:
  %t64_2866 = load i64, i64* %param_ctx_2599
  %fn_ret_2867 = call i64 @__omni_cyw_priv_tcp_send_pure_ack(i64 %t64_2866, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  br label %if_merge_120

if_else_120:
  call void @__omni_c_return_ptr(i8* null)
  %t64_2868 = load i64, i64* %param_ctx_2599
  %t_2869 = add i64 %t64_2868, 8000
  %t_2870 = add i64 %t_2869, 200
  store i64 %t_2870, i64* %var_key_tmp_2871
  call void @__omni_c_return_ptr(i8* null)
  %t64_2872 = load i64, i64* %param_ctx_2599
  %t_2873 = add i64 %t64_2872, 8000
  %t_2874 = add i64 %t_2873, 264
  store i64 %t_2874, i64* %var_acc_tmp_2875
  call void @__omni_c_return_ptr(i8* null)
  %t64_2876 = load i64, i64* %param_ctx_2599
  %t64_2877 = load i64, i64* %var_hbuf_2835
  %t64_2878 = load i64, i64* %var_hend_2863
  %t64_2879 = load i64, i64* %var_key_tmp_2871
  %fn_ret_2880 = call i64 @__omni_cyw_priv_http_extract_ws_key(i64 %t64_2876, i64 %t64_2877, i64 %t64_2878, i64 %t64_2879, i64 -581005069656919554, i64 -581005069656919554)
  store i64 %fn_ret_2880, i64* %var_klen_2881
  %t64_2882 = load i64, i64* %var_klen_2881
  %cond_2883 = icmp sgt i64 %t64_2882, 0
  br i1 %cond_2883, label %if_then_121, label %if_else_121

if_then_121:
  %t64_2884 = load i64, i64* %param_ctx_2599
  %t64_2885 = load i64, i64* %var_key_tmp_2871
  %t64_2886 = load i64, i64* %var_klen_2881
  %t64_2887 = load i64, i64* %var_acc_tmp_2875
  %fn_ret_2888 = call i64 @__omni_fn_cyw_ws_compute_accept(i64 %t64_2884, i64 %t64_2885, i64 %t64_2886, i64 %t64_2887, i64 -581005069656919554, i64 -581005069656919554)
  call void @__omni_c_return_ptr(i8* null)
  %t64_2889 = load i64, i64* %param_ctx_2599
  %t_2890 = add i64 %t64_2889, 6464
  store i64 %t_2890, i64* %var_resp_2891
  call void @__omni_c_return_ptr(i8* null)
  store i64 0, i64* %var_rp_2892
  call void @__omni_c_return_ptr(i8* null)
  %t64_2893 = load i64, i64* %var_rp_2892
  %t64_2894 = load i64, i64* %var_resp_2891
  %t64_2895 = load i64, i64* %var_rp_2892
  %t_2896 = add i64 %t64_2894, %t64_2895
  %str_lit_2897 = getelementptr inbounds [98 x i8], [98 x i8]* @.str.cyw_9, i64 0, i64 0
  call void @__omni_c_return_ptr(i8* %str_lit_2897)
  %p2i64_2898 = ptrtoint i8* %str_lit_2897 to i64
  %fn_ret_2899 = call i64 @__omni_cyw_priv_put_ascii(i64 %t_2896, i64 %p2i64_2898, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t_2900 = add i64 %t64_2893, %fn_ret_2899
  store i64 %t_2900, i64* %var_rp_2892
  %t64_2901 = load i64, i64* %var_resp_2891
  %t64_2902 = load i64, i64* %var_rp_2892
  %t_2903 = add i64 %t64_2901, %t64_2902
  %t64_2904 = load i64, i64* %var_acc_tmp_2875
  %fn_ret_2905 = call i64 @__omni_cyw_priv_bcopy(i64 %t_2903, i64 %t64_2904, i64 28, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  call void @__omni_c_return_ptr(i8* null)
  %t64_2906 = load i64, i64* %var_rp_2892
  %t_2907 = add i64 %t64_2906, 28
  store i64 %t_2907, i64* %var_rp_2892
  call void @__omni_c_return_ptr(i8* null)
  %t64_2908 = load i64, i64* %var_rp_2892
  %t64_2909 = load i64, i64* %var_resp_2891
  %t64_2910 = load i64, i64* %var_rp_2892
  %t_2911 = add i64 %t64_2909, %t64_2910
  %str_lit_2912 = getelementptr inbounds [5 x i8], [5 x i8]* @.str.cyw_10, i64 0, i64 0
  call void @__omni_c_return_ptr(i8* %str_lit_2912)
  %p2i64_2913 = ptrtoint i8* %str_lit_2912 to i64
  %fn_ret_2914 = call i64 @__omni_cyw_priv_put_ascii(i64 %t_2911, i64 %p2i64_2913, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t_2915 = add i64 %t64_2908, %fn_ret_2914
  store i64 %t_2915, i64* %var_rp_2892
  %t64_2916 = load i64, i64* %param_ctx_2599
  %t_2917 = add i64 %t64_2916, 12
  %ext_i64_2918 = call i64 @mmio_write32(i64 %t_2917, i64 4)
  call void @__omni_c_return_ptr(i8* null)
  store i64 4, i64* %var_state_2670
  %t64_2919 = load i64, i64* %param_ctx_2599
  %t64_2920 = load i64, i64* %var_resp_2891
  %t64_2921 = load i64, i64* %var_rp_2892
  %fn_ret_2922 = call i64 @__omni_cyw_priv_tcp_send_tracked(i64 %t64_2919, i64 24, i64 %t64_2920, i64 %t64_2921, i64 -581005069656919554, i64 -581005069656919554)
  %t64_2923 = load i64, i64* %var_hlen_2832
  %t64_2924 = load i64, i64* %var_hend_2863
  %cond_2925 = icmp sgt i64 %t64_2923, %t64_2924
  br i1 %cond_2925, label %if_then_122, label %if_merge_122

if_then_122:
  %t64_2926 = load i64, i64* %param_ctx_2599
  %t64_2927 = load i64, i64* %var_hbuf_2835
  %t64_2928 = load i64, i64* %var_hend_2863
  %t_2929 = add i64 %t64_2927, %t64_2928
  %t64_2930 = load i64, i64* %var_hlen_2832
  %t64_2931 = load i64, i64* %var_hend_2863
  %t_2932 = sub i64 %t64_2930, %t64_2931
  %fn_ret_2933 = call i64 @__omni_cyw_priv_ws_handle_frames(i64 %t64_2926, i64 %t_2929, i64 %t_2932, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  br label %if_merge_122

if_merge_122:
  br label %if_merge_121

if_else_121:
  %t64_2934 = load i64, i64* %param_ctx_2599
  %fn_ret_2935 = call i64 @__omni_cyw_priv_tcp_send_tracked(i64 %t64_2934, i64 17, i64 0, i64 0, i64 -581005069656919554, i64 -581005069656919554)
  %t64_2936 = load i64, i64* %param_ctx_2599
  %t_2937 = add i64 %t64_2936, 12
  %ext_i64_2938 = call i64 @mmio_write32(i64 %t_2937, i64 1)
  br label %if_merge_121

if_merge_121:
  br label %if_merge_120

if_merge_120:
  br label %if_merge_117

if_else_117:
  %t64_2939 = load i64, i64* %var_state_2670
  %cond_2940 = icmp eq i64 %t64_2939, 4
  br i1 %cond_2940, label %if_then_123, label %if_merge_123

if_then_123:
  %t64_2941 = load i64, i64* %param_ctx_2599
  %fn_ret_2942 = call i64 @__omni_cyw_priv_tcp_send_pure_ack(i64 %t64_2941, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_2943 = load i64, i64* %param_ctx_2599
  %t_2944 = add i64 %t64_2943, 124
  %ext_i64_2945 = call i64 @mmio_read32(i64 %t_2944)
  %cond_2946 = icmp ne i64 %ext_i64_2945, 0
  br i1 %cond_2946, label %if_then_124, label %if_else_124

if_then_124:
  call void @__omni_c_return_ptr(i8* null)
  %t64_2947 = load i64, i64* %var_data_len_2666
  store i64 %t64_2947, i64* %var_cp_2948
  %t64_2949 = load i64, i64* %var_cp_2948
  %cond_2950 = icmp sgt i64 %t64_2949, 1020
  br i1 %cond_2950, label %if_then_125, label %if_merge_125

if_then_125:
  call void @__omni_c_return_ptr(i8* null)
  store i64 1020, i64* %var_cp_2948
  br label %if_merge_125

if_merge_125:
  %t64_2951 = load i64, i64* %param_ctx_2599
  %t_2952 = add i64 %t64_2951, 1184
  %t64_2953 = load i64, i64* %var_data_ptr_2662
  %t64_2954 = load i64, i64* %var_cp_2948
  %fn_ret_2955 = call i64 @__omni_cyw_priv_bcopy(i64 %t_2952, i64 %t64_2953, i64 %t64_2954, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_2956 = load i64, i64* %param_ctx_2599
  %t_2957 = add i64 %t64_2956, 1184
  %t64_2958 = load i64, i64* %var_cp_2948
  %t_2959 = add i64 %t_2957, %t64_2958
  %fn_ret_2960 = call i64 @__omni_cyw_priv_w8(i64 %t_2959, i64 0, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_2961 = load i64, i64* %param_ctx_2599
  %t_2962 = add i64 %t64_2961, 120
  %ext_i64_2963 = call i64 @mmio_write32(i64 %t_2962, i64 1)
  br label %if_merge_124

if_else_124:
  %t64_2964 = load i64, i64* %param_ctx_2599
  %t64_2965 = load i64, i64* %var_data_ptr_2662
  %t64_2966 = load i64, i64* %var_data_len_2666
  %fn_ret_2967 = call i64 @__omni_cyw_priv_ws_handle_frames(i64 %t64_2964, i64 %t64_2965, i64 %t64_2966, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  br label %if_merge_124

if_merge_124:
  br label %if_merge_123

if_merge_123:
  br label %if_merge_117

if_merge_117:
  br label %if_merge_115

if_merge_115:
  %t64_2968 = load i64, i64* %var_flags_2658
  %t_2969 = and i64 %t64_2968, 1
  %cond_2970 = icmp ne i64 %t_2969, 0
  br i1 %cond_2970, label %if_then_126, label %if_merge_126

if_then_126:
  call void @__omni_c_return_ptr(i8* null)
  %t64_2971 = load i64, i64* %param_ctx_2599
  %t_2972 = add i64 %t64_2971, 92
  %ext_i64_2973 = call i64 @mmio_read32(i64 %t_2972)
  store i64 %ext_i64_2973, i64* %var_cur_ack_2974
  %t64_2975 = load i64, i64* %param_ctx_2599
  %t_2976 = add i64 %t64_2975, 92
  %t64_2977 = load i64, i64* %var_cur_ack_2974
  %t_2978 = add i64 %t64_2977, 1
  %t_2979 = and i64 %t_2978, 4294967295
  %ext_i64_2980 = call i64 @mmio_write32(i64 %t_2976, i64 %t_2979)
  %t64_2981 = load i64, i64* %param_ctx_2599
  %t64_2982 = load i64, i64* %param_ctx_2599
  %t_2983 = add i64 %t64_2982, 72
  %t64_2984 = load i64, i64* %param_ctx_2599
  %t_2985 = add i64 %t64_2984, 80
  %t64_2986 = load i64, i64* %var_sport_2621
  %t64_2987 = load i64, i64* %var_ws_port_2629
  %t64_2988 = load i64, i64* %param_ctx_2599
  %t_2989 = add i64 %t64_2988, 88
  %ext_i64_2990 = call i64 @mmio_read32(i64 %t_2989)
  %t64_2991 = load i64, i64* %param_ctx_2599
  %t_2992 = add i64 %t64_2991, 92
  %ext_i64_2993 = call i64 @mmio_read32(i64 %t_2992)
  %fn_ret_2994 = call i64 @__omni_cyw_priv_tcp_send_segment(i64 %t64_2981, i64 %t_2983, i64 %t_2985, i64 %t64_2986, i64 %t64_2987, i64 %ext_i64_2990, i64 %ext_i64_2993, i64 17, i64 0, i64 0)
  %t64_2995 = load i64, i64* %param_ctx_2599
  %t_2996 = add i64 %t64_2995, 88
  %t64_2997 = load i64, i64* %param_ctx_2599
  %t_2998 = add i64 %t64_2997, 88
  %ext_i64_2999 = call i64 @mmio_read32(i64 %t_2998)
  %t_3000 = add i64 %ext_i64_2999, 1
  %t_3001 = and i64 %t_3000, 4294967295
  %ext_i64_3002 = call i64 @mmio_write32(i64 %t_2996, i64 %t_3001)
  %t64_3003 = load i64, i64* %param_ctx_2599
  %t_3004 = add i64 %t64_3003, 12
  %ext_i64_3005 = call i64 @mmio_write32(i64 %t_3004, i64 1)
  %t64_3006 = load i64, i64* %param_ctx_2599
  %t_3007 = add i64 %t64_3006, 100
  %ext_i64_3008 = call i64 @mmio_write32(i64 %t_3007, i64 0)
  br label %if_merge_126

if_merge_126:
  ret i64 1
}


define void @__omni_trampoline_tcp_ws_rx(i8* %raw_arg) {
entry:
  %raw_int = ptrtoint i8* %raw_arg to i64
  %res = call i64 @__omni_cyw_priv_tcp_ws_rx(i64 %raw_int, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  ret void
}

define void @__omni_trampoline_pack_tcp_ws_rx(i8* %raw_pack) {
entry:
  %p64 = bitcast i8* %raw_pack to i64*
  %p0 = getelementptr inbounds i64, i64* %p64, i64 0
  %a0 = load i64, i64* %p0
  %p1 = getelementptr inbounds i64, i64* %p64, i64 1
  %a1 = load i64, i64* %p1
  %p2 = getelementptr inbounds i64, i64* %p64, i64 2
  %a2 = load i64, i64* %p2
  %p3 = getelementptr inbounds i64, i64* %p64, i64 3
  %a3 = load i64, i64* %p3
  %p4 = getelementptr inbounds i64, i64* %p64, i64 4
  %a4 = load i64, i64* %p4
  %p5 = getelementptr inbounds i64, i64* %p64, i64 5
  %a5 = load i64, i64* %p5
  call void @free(i8* %raw_pack)
  %res = call i64 @__omni_cyw_priv_tcp_ws_rx(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5)
  ret void
}

define i64 @__omni_cyw_priv_eth_rx_dispatch(i64 %arg0, i64 %arg1, i64 %arg2, i64 %arg3, i64 %arg4, i64 %arg5) {
entry:
  %param_ctx_3010 = alloca i64
  %param_eth_3012 = alloca i64
  %param_eth_len_3014 = alloca i64
  %var_my_mac_3019 = alloca i64
  %var_my_ip_3022 = alloca i64
  %var_etype_3035 = alloca i64
  %var_op_3045 = alloca i64
  %var_ip_3074 = alloca i64
  %var_ihl_3083 = alloca i64
  %var_tot_len_3087 = alloca i64
  %var_proto_3104 = alloca i64
  %var_icmp_3122 = alloca i64
  %var_icmp_len_3126 = alloca i64
  %var_reply_3137 = alloca i64
  %var_ics_3167 = alloca i64
  %var_udp_3188 = alloca i64
  %var_sport_3192 = alloca i64
  %var_dport_3196 = alloca i64
  %var_ulen_3200 = alloca i64
  %param_unset_ctx_3009 = icmp eq i64 %arg0, -581005069656919554
  %param_dsel_ctx_3009 = select i1 %param_unset_ctx_3009, i64 0, i64 %arg0
  store i64 %param_dsel_ctx_3009, i64* %param_ctx_3010
  %param_unset_eth_3011 = icmp eq i64 %arg1, -581005069656919554
  %param_dsel_eth_3011 = select i1 %param_unset_eth_3011, i64 0, i64 %arg1
  store i64 %param_dsel_eth_3011, i64* %param_eth_3012
  %param_unset_eth_len_3013 = icmp eq i64 %arg2, -581005069656919554
  %param_dsel_eth_len_3013 = select i1 %param_unset_eth_len_3013, i64 0, i64 %arg2
  store i64 %param_dsel_eth_len_3013, i64* %param_eth_len_3014
  %t64_3015 = load i64, i64* %param_eth_len_3014
  %cond_3016 = icmp slt i64 %t64_3015, 14
  br i1 %cond_3016, label %if_then_127, label %if_merge_127

if_then_127:
  ret i64 0
if_merge_127:
  call void @__omni_c_return_ptr(i8* null)
  %t64_3017 = load i64, i64* %param_ctx_3010
  %t_3018 = add i64 %t64_3017, 32
  store i64 %t_3018, i64* %var_my_mac_3019
  call void @__omni_c_return_ptr(i8* null)
  %t64_3020 = load i64, i64* %param_ctx_3010
  %t_3021 = add i64 %t64_3020, 40
  store i64 %t_3021, i64* %var_my_ip_3022
  %t64_3024 = load i64, i64* %param_eth_3012
  %fn_ret_3025 = call i64 @__omni_cyw_priv_mac_is_bcast(i64 %t64_3024, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %cond_3026 = icmp eq i64 %fn_ret_3025, 0
  br i1 %cond_3026, label %sc_and_rhs_3023, label %sc_and_short_3023

sc_and_short_3023:
  br label %sc_and_merge_3023

sc_and_rhs_3023:
  %t64_3027 = load i64, i64* %param_eth_3012
  %t64_3028 = load i64, i64* %var_my_mac_3019
  %fn_ret_3029 = call i64 @__omni_cyw_priv_mac_eq(i64 %t64_3027, i64 %t64_3028, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %cond_3030 = icmp eq i64 %fn_ret_3029, 0
  br label %sc_and_done_3023

sc_and_done_3023:
  br label %sc_and_merge_3023

sc_and_merge_3023:
  %and_cond_3031 = phi i1 [ false, %sc_and_short_3023 ], [ %cond_3030, %sc_and_done_3023 ]
  br i1 %and_cond_3031, label %if_then_128, label %if_merge_128

if_then_128:
  ret i64 0
if_merge_128:
  call void @__omni_c_return_ptr(i8* null)
  %t64_3032 = load i64, i64* %param_eth_3012
  %t_3033 = add i64 %t64_3032, 12
  %fn_ret_3034 = call i64 @__omni_cyw_priv_r16_be(i64 %t_3033, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  store i64 %fn_ret_3034, i64* %var_etype_3035
  %t64_3037 = load i64, i64* %var_etype_3035
  %cond_3038 = icmp eq i64 %t64_3037, 2054
  br i1 %cond_3038, label %sc_and_rhs_3036, label %sc_and_short_3036

sc_and_short_3036:
  br label %sc_and_merge_3036

sc_and_rhs_3036:
  %t64_3039 = load i64, i64* %param_eth_len_3014
  %cond_3040 = icmp sge i64 %t64_3039, 42
  br label %sc_and_done_3036

sc_and_done_3036:
  br label %sc_and_merge_3036

sc_and_merge_3036:
  %and_cond_3041 = phi i1 [ false, %sc_and_short_3036 ], [ %cond_3040, %sc_and_done_3036 ]
  br i1 %and_cond_3041, label %if_then_129, label %if_merge_129

if_then_129:
  call void @__omni_c_return_ptr(i8* null)
  %t64_3042 = load i64, i64* %param_eth_3012
  %t_3043 = add i64 %t64_3042, 20
  %fn_ret_3044 = call i64 @__omni_cyw_priv_r16_be(i64 %t_3043, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  store i64 %fn_ret_3044, i64* %var_op_3045
  %t64_3048 = load i64, i64* %var_op_3045
  %cond_3049 = icmp eq i64 %t64_3048, 1
  br i1 %cond_3049, label %sc_and_rhs_3047, label %sc_and_short_3047

sc_and_short_3047:
  br label %sc_and_merge_3047

sc_and_rhs_3047:
  %t64_3050 = load i64, i64* %var_my_ip_3022
  %fn_ret_3051 = call i64 @__omni_cyw_priv_ip_is_zero(i64 %t64_3050, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %cond_3052 = icmp eq i64 %fn_ret_3051, 0
  br label %sc_and_done_3047

sc_and_done_3047:
  br label %sc_and_merge_3047

sc_and_merge_3047:
  %and_cond_3053 = phi i1 [ false, %sc_and_short_3047 ], [ %cond_3052, %sc_and_done_3047 ]
  br i1 %and_cond_3053, label %sc_and_rhs_3046, label %sc_and_short_3046

sc_and_short_3046:
  br label %sc_and_merge_3046

sc_and_rhs_3046:
  %t64_3054 = load i64, i64* %param_eth_3012
  %t_3055 = add i64 %t64_3054, 38
  %t64_3056 = load i64, i64* %var_my_ip_3022
  %fn_ret_3057 = call i64 @__omni_cyw_priv_ip_eq(i64 %t_3055, i64 %t64_3056, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %cond_3058 = icmp ne i64 %fn_ret_3057, 0
  br label %sc_and_done_3046

sc_and_done_3046:
  br label %sc_and_merge_3046

sc_and_merge_3046:
  %and_cond_3059 = phi i1 [ false, %sc_and_short_3046 ], [ %cond_3058, %sc_and_done_3046 ]
  br i1 %and_cond_3059, label %if_then_130, label %if_merge_130

if_then_130:
  %t64_3060 = load i64, i64* %param_ctx_3010
  %t64_3061 = load i64, i64* %param_eth_3012
  %t_3062 = add i64 %t64_3061, 6
  %t64_3063 = load i64, i64* %param_eth_3012
  %t_3064 = add i64 %t64_3063, 28
  %fn_ret_3065 = call i64 @__omni_cyw_priv_send_arp_pkt(i64 %t64_3060, i64 %t_3062, i64 %t_3064, i64 2, i64 -581005069656919554, i64 -581005069656919554)
  ret i64 1
if_merge_130:
  ret i64 0
if_merge_129:
  %t64_3067 = load i64, i64* %var_etype_3035
  %cond_3068 = icmp ne i64 %t64_3067, 2048
  br i1 %cond_3068, label %sc_or_short_3066, label %sc_or_rhs_3066

sc_or_short_3066:
  br label %sc_or_merge_3066

sc_or_rhs_3066:
  %t64_3069 = load i64, i64* %param_eth_len_3014
  %cond_3070 = icmp slt i64 %t64_3069, 34
  br label %sc_or_done_3066

sc_or_done_3066:
  br label %sc_or_merge_3066

sc_or_merge_3066:
  %or_cond_3071 = phi i1 [ true, %sc_or_short_3066 ], [ %cond_3070, %sc_or_done_3066 ]
  br i1 %or_cond_3071, label %if_then_131, label %if_merge_131

if_then_131:
  ret i64 0
if_merge_131:
  call void @__omni_c_return_ptr(i8* null)
  %t64_3072 = load i64, i64* %param_eth_3012
  %t_3073 = add i64 %t64_3072, 14
  store i64 %t_3073, i64* %var_ip_3074
  %t64_3075 = load i64, i64* %var_ip_3074
  %fn_ret_3076 = call i64 @__omni_cyw_priv_r8(i64 %t64_3075, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t_3077 = ashr i64 %fn_ret_3076, 4
  %cond_3078 = icmp ne i64 %t_3077, 4
  br i1 %cond_3078, label %if_then_132, label %if_merge_132

if_then_132:
  ret i64 0
if_merge_132:
  call void @__omni_c_return_ptr(i8* null)
  %t64_3079 = load i64, i64* %var_ip_3074
  %fn_ret_3080 = call i64 @__omni_cyw_priv_r8(i64 %t64_3079, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t_3081 = and i64 %fn_ret_3080, 15
  %t_3082 = mul i64 %t_3081, 4
  store i64 %t_3082, i64* %var_ihl_3083
  call void @__omni_c_return_ptr(i8* null)
  %t64_3084 = load i64, i64* %var_ip_3074
  %t_3085 = add i64 %t64_3084, 2
  %fn_ret_3086 = call i64 @__omni_cyw_priv_r16_be(i64 %t_3085, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  store i64 %fn_ret_3086, i64* %var_tot_len_3087
  %t64_3090 = load i64, i64* %var_ihl_3083
  %cond_3091 = icmp slt i64 %t64_3090, 20
  br i1 %cond_3091, label %sc_or_short_3089, label %sc_or_rhs_3089

sc_or_short_3089:
  br label %sc_or_merge_3089

sc_or_rhs_3089:
  %t64_3092 = load i64, i64* %var_tot_len_3087
  %t64_3093 = load i64, i64* %var_ihl_3083
  %cond_3094 = icmp slt i64 %t64_3092, %t64_3093
  br label %sc_or_done_3089

sc_or_done_3089:
  br label %sc_or_merge_3089

sc_or_merge_3089:
  %or_cond_3095 = phi i1 [ true, %sc_or_short_3089 ], [ %cond_3094, %sc_or_done_3089 ]
  br i1 %or_cond_3095, label %sc_or_short_3088, label %sc_or_rhs_3088

sc_or_short_3088:
  br label %sc_or_merge_3088

sc_or_rhs_3088:
  %t64_3096 = load i64, i64* %var_tot_len_3087
  %t_3097 = add i64 14, %t64_3096
  %t64_3098 = load i64, i64* %param_eth_len_3014
  %cond_3099 = icmp sgt i64 %t_3097, %t64_3098
  br label %sc_or_done_3088

sc_or_done_3088:
  br label %sc_or_merge_3088

sc_or_merge_3088:
  %or_cond_3100 = phi i1 [ true, %sc_or_short_3088 ], [ %cond_3099, %sc_or_done_3088 ]
  br i1 %or_cond_3100, label %if_then_133, label %if_merge_133

if_then_133:
  ret i64 0
if_merge_133:
  call void @__omni_c_return_ptr(i8* null)
  %t64_3101 = load i64, i64* %var_ip_3074
  %t_3102 = add i64 %t64_3101, 9
  %fn_ret_3103 = call i64 @__omni_cyw_priv_r8(i64 %t_3102, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  store i64 %fn_ret_3103, i64* %var_proto_3104
  %t64_3107 = load i64, i64* %var_proto_3104
  %cond_3108 = icmp eq i64 %t64_3107, 1
  br i1 %cond_3108, label %sc_and_rhs_3106, label %sc_and_short_3106

sc_and_short_3106:
  br label %sc_and_merge_3106

sc_and_rhs_3106:
  %t64_3109 = load i64, i64* %var_my_ip_3022
  %fn_ret_3110 = call i64 @__omni_cyw_priv_ip_is_zero(i64 %t64_3109, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %cond_3111 = icmp eq i64 %fn_ret_3110, 0
  br label %sc_and_done_3106

sc_and_done_3106:
  br label %sc_and_merge_3106

sc_and_merge_3106:
  %and_cond_3112 = phi i1 [ false, %sc_and_short_3106 ], [ %cond_3111, %sc_and_done_3106 ]
  br i1 %and_cond_3112, label %sc_and_rhs_3105, label %sc_and_short_3105

sc_and_short_3105:
  br label %sc_and_merge_3105

sc_and_rhs_3105:
  %t64_3113 = load i64, i64* %var_ip_3074
  %t_3114 = add i64 %t64_3113, 16
  %t64_3115 = load i64, i64* %var_my_ip_3022
  %fn_ret_3116 = call i64 @__omni_cyw_priv_ip_eq(i64 %t_3114, i64 %t64_3115, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %cond_3117 = icmp ne i64 %fn_ret_3116, 0
  br label %sc_and_done_3105

sc_and_done_3105:
  br label %sc_and_merge_3105

sc_and_merge_3105:
  %and_cond_3118 = phi i1 [ false, %sc_and_short_3105 ], [ %cond_3117, %sc_and_done_3105 ]
  br i1 %and_cond_3118, label %if_then_134, label %if_merge_134

if_then_134:
  call void @__omni_c_return_ptr(i8* null)
  %t64_3119 = load i64, i64* %var_ip_3074
  %t64_3120 = load i64, i64* %var_ihl_3083
  %t_3121 = add i64 %t64_3119, %t64_3120
  store i64 %t_3121, i64* %var_icmp_3122
  call void @__omni_c_return_ptr(i8* null)
  %t64_3123 = load i64, i64* %var_tot_len_3087
  %t64_3124 = load i64, i64* %var_ihl_3083
  %t_3125 = sub i64 %t64_3123, %t64_3124
  store i64 %t_3125, i64* %var_icmp_len_3126
  %t64_3128 = load i64, i64* %var_icmp_len_3126
  %cond_3129 = icmp sge i64 %t64_3128, 8
  br i1 %cond_3129, label %sc_and_rhs_3127, label %sc_and_short_3127

sc_and_short_3127:
  br label %sc_and_merge_3127

sc_and_rhs_3127:
  %t64_3130 = load i64, i64* %var_icmp_3122
  %fn_ret_3131 = call i64 @__omni_cyw_priv_r8(i64 %t64_3130, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %cond_3132 = icmp eq i64 %fn_ret_3131, 8
  br label %sc_and_done_3127

sc_and_done_3127:
  br label %sc_and_merge_3127

sc_and_merge_3127:
  %and_cond_3133 = phi i1 [ false, %sc_and_short_3127 ], [ %cond_3132, %sc_and_done_3127 ]
  br i1 %and_cond_3133, label %if_then_135, label %if_merge_135

if_then_135:
  call void @__omni_c_return_ptr(i8* null)
  %t64_3134 = load i64, i64* %param_ctx_3010
  %t_3135 = add i64 %t64_3134, 4672
  %t_3136 = add i64 %t_3135, 18
  store i64 %t_3136, i64* %var_reply_3137
  %t64_3138 = load i64, i64* %var_reply_3137
  %t64_3139 = load i64, i64* %param_eth_3012
  %t_3140 = add i64 %t64_3139, 6
  %t64_3141 = load i64, i64* %var_my_mac_3019
  %fn_ret_3142 = call i64 @__omni_cyw_priv_build_eth_hdr(i64 %t64_3138, i64 %t_3140, i64 %t64_3141, i64 2048, i64 -581005069656919554, i64 -581005069656919554)
  %t64_3143 = load i64, i64* %param_ctx_3010
  %t64_3144 = load i64, i64* %var_reply_3137
  %t_3145 = add i64 %t64_3144, 14
  %t64_3146 = load i64, i64* %var_my_ip_3022
  %t64_3147 = load i64, i64* %var_ip_3074
  %t_3148 = add i64 %t64_3147, 12
  %t64_3149 = load i64, i64* %var_icmp_len_3126
  %fn_ret_3150 = call i64 @__omni_cyw_priv_build_ipv4_hdr(i64 %t64_3143, i64 %t_3145, i64 %t64_3146, i64 %t_3148, i64 1, i64 %t64_3149)
  %t64_3151 = load i64, i64* %var_reply_3137
  %t_3152 = add i64 %t64_3151, 34
  %t64_3153 = load i64, i64* %var_icmp_3122
  %t64_3154 = load i64, i64* %var_icmp_len_3126
  %fn_ret_3155 = call i64 @__omni_cyw_priv_bcopy(i64 %t_3152, i64 %t64_3153, i64 %t64_3154, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_3156 = load i64, i64* %var_reply_3137
  %t_3157 = add i64 %t64_3156, 34
  %fn_ret_3158 = call i64 @__omni_cyw_priv_w8(i64 %t_3157, i64 0, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_3159 = load i64, i64* %var_reply_3137
  %t_3160 = add i64 %t64_3159, 36
  %fn_ret_3161 = call i64 @__omni_cyw_priv_w16_be(i64 %t_3160, i64 0, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  call void @__omni_c_return_ptr(i8* null)
  %t64_3162 = load i64, i64* %var_reply_3137
  %t_3163 = add i64 %t64_3162, 34
  %t64_3164 = load i64, i64* %var_icmp_len_3126
  %fn_ret_3165 = call i64 @__omni_cyw_priv_inet_csum_acc(i64 0, i64 %t_3163, i64 %t64_3164, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %fn_ret_3166 = call i64 @__omni_cyw_priv_inet_csum_finish(i64 %fn_ret_3165, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  store i64 %fn_ret_3166, i64* %var_ics_3167
  %t64_3168 = load i64, i64* %var_reply_3137
  %t_3169 = add i64 %t64_3168, 36
  %t64_3170 = load i64, i64* %var_ics_3167
  %fn_ret_3171 = call i64 @__omni_cyw_priv_w16_be(i64 %t_3169, i64 %t64_3170, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_3172 = load i64, i64* %param_ctx_3010
  %t64_3173 = load i64, i64* %var_reply_3137
  %t64_3174 = load i64, i64* %var_icmp_len_3126
  %t_3175 = add i64 34, %t64_3174
  %fn_ret_3176 = call i64 @__omni_fn_cyw_tx_eth(i64 %t64_3172, i64 %t64_3173, i64 %t_3175, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  ret i64 1
if_merge_135:
  ret i64 0
if_merge_134:
  %t64_3178 = load i64, i64* %var_proto_3104
  %cond_3179 = icmp eq i64 %t64_3178, 17
  br i1 %cond_3179, label %sc_and_rhs_3177, label %sc_and_short_3177

sc_and_short_3177:
  br label %sc_and_merge_3177

sc_and_rhs_3177:
  %t64_3180 = load i64, i64* %var_tot_len_3087
  %t64_3181 = load i64, i64* %var_ihl_3083
  %t_3182 = add i64 %t64_3181, 8
  %cond_3183 = icmp sge i64 %t64_3180, %t_3182
  br label %sc_and_done_3177

sc_and_done_3177:
  br label %sc_and_merge_3177

sc_and_merge_3177:
  %and_cond_3184 = phi i1 [ false, %sc_and_short_3177 ], [ %cond_3183, %sc_and_done_3177 ]
  br i1 %and_cond_3184, label %if_then_136, label %if_merge_136

if_then_136:
  call void @__omni_c_return_ptr(i8* null)
  %t64_3185 = load i64, i64* %var_ip_3074
  %t64_3186 = load i64, i64* %var_ihl_3083
  %t_3187 = add i64 %t64_3185, %t64_3186
  store i64 %t_3187, i64* %var_udp_3188
  call void @__omni_c_return_ptr(i8* null)
  %t64_3189 = load i64, i64* %var_udp_3188
  %t_3190 = add i64 %t64_3189, 0
  %fn_ret_3191 = call i64 @__omni_cyw_priv_r16_be(i64 %t_3190, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  store i64 %fn_ret_3191, i64* %var_sport_3192
  call void @__omni_c_return_ptr(i8* null)
  %t64_3193 = load i64, i64* %var_udp_3188
  %t_3194 = add i64 %t64_3193, 2
  %fn_ret_3195 = call i64 @__omni_cyw_priv_r16_be(i64 %t_3194, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  store i64 %fn_ret_3195, i64* %var_dport_3196
  call void @__omni_c_return_ptr(i8* null)
  %t64_3197 = load i64, i64* %var_udp_3188
  %t_3198 = add i64 %t64_3197, 4
  %fn_ret_3199 = call i64 @__omni_cyw_priv_r16_be(i64 %t_3198, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  store i64 %fn_ret_3199, i64* %var_ulen_3200
  %t64_3202 = load i64, i64* %var_ulen_3200
  %cond_3203 = icmp slt i64 %t64_3202, 8
  br i1 %cond_3203, label %sc_or_short_3201, label %sc_or_rhs_3201

sc_or_short_3201:
  br label %sc_or_merge_3201

sc_or_rhs_3201:
  %t64_3204 = load i64, i64* %var_ihl_3083
  %t64_3205 = load i64, i64* %var_ulen_3200
  %t_3206 = add i64 %t64_3204, %t64_3205
  %t64_3207 = load i64, i64* %var_tot_len_3087
  %cond_3208 = icmp sgt i64 %t_3206, %t64_3207
  br label %sc_or_done_3201

sc_or_done_3201:
  br label %sc_or_merge_3201

sc_or_merge_3201:
  %or_cond_3209 = phi i1 [ true, %sc_or_short_3201 ], [ %cond_3208, %sc_or_done_3201 ]
  br i1 %or_cond_3209, label %if_then_137, label %if_merge_137

if_then_137:
  ret i64 0
if_merge_137:
  %t64_3210 = load i64, i64* %var_dport_3196
  %cond_3211 = icmp eq i64 %t64_3210, 68
  br i1 %cond_3211, label %if_then_138, label %if_merge_138

if_then_138:
  %t64_3212 = load i64, i64* %param_ctx_3010
  %t64_3213 = load i64, i64* %param_eth_3012
  %t64_3214 = load i64, i64* %var_udp_3188
  %t_3215 = add i64 %t64_3214, 8
  %t64_3216 = load i64, i64* %var_ulen_3200
  %t_3217 = sub i64 %t64_3216, 8
  %fn_ret_3218 = call i64 @__omni_cyw_priv_dhcp_rx_handle(i64 %t64_3212, i64 %t64_3213, i64 %t_3215, i64 %t_3217, i64 -581005069656919554, i64 -581005069656919554)
  ret i64 %fn_ret_3218
if_merge_138:
  %t64_3220 = load i64, i64* %var_dport_3196
  %cond_3221 = icmp eq i64 %t64_3220, 1425
  br i1 %cond_3221, label %sc_and_rhs_3219, label %sc_and_short_3219

sc_and_short_3219:
  br label %sc_and_merge_3219

sc_and_rhs_3219:
  %t64_3222 = load i64, i64* %var_my_ip_3022
  %fn_ret_3223 = call i64 @__omni_cyw_priv_ip_is_zero(i64 %t64_3222, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %cond_3224 = icmp eq i64 %fn_ret_3223, 0
  br label %sc_and_done_3219

sc_and_done_3219:
  br label %sc_and_merge_3219

sc_and_merge_3219:
  %and_cond_3225 = phi i1 [ false, %sc_and_short_3219 ], [ %cond_3224, %sc_and_done_3219 ]
  br i1 %and_cond_3225, label %if_then_139, label %if_merge_139

if_then_139:
  %t64_3226 = load i64, i64* %param_ctx_3010
  %t64_3227 = load i64, i64* %param_eth_3012
  %t_3228 = add i64 %t64_3227, 6
  %t64_3229 = load i64, i64* %var_ip_3074
  %t_3230 = add i64 %t64_3229, 12
  %t64_3231 = load i64, i64* %var_sport_3192
  %fn_ret_3232 = call i64 @__omni_cyw_priv_send_discovery_beacon(i64 %t64_3226, i64 %t_3228, i64 %t_3230, i64 %t64_3231, i64 -581005069656919554, i64 -581005069656919554)
  ret i64 1
if_merge_139:
  ret i64 0
if_merge_136:
  %t64_3235 = load i64, i64* %var_proto_3104
  %cond_3236 = icmp eq i64 %t64_3235, 6
  br i1 %cond_3236, label %sc_and_rhs_3234, label %sc_and_short_3234

sc_and_short_3234:
  br label %sc_and_merge_3234

sc_and_rhs_3234:
  %t64_3237 = load i64, i64* %var_my_ip_3022
  %fn_ret_3238 = call i64 @__omni_cyw_priv_ip_is_zero(i64 %t64_3237, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %cond_3239 = icmp eq i64 %fn_ret_3238, 0
  br label %sc_and_done_3234

sc_and_done_3234:
  br label %sc_and_merge_3234

sc_and_merge_3234:
  %and_cond_3240 = phi i1 [ false, %sc_and_short_3234 ], [ %cond_3239, %sc_and_done_3234 ]
  br i1 %and_cond_3240, label %sc_and_rhs_3233, label %sc_and_short_3233

sc_and_short_3233:
  br label %sc_and_merge_3233

sc_and_rhs_3233:
  %t64_3241 = load i64, i64* %var_ip_3074
  %t_3242 = add i64 %t64_3241, 16
  %t64_3243 = load i64, i64* %var_my_ip_3022
  %fn_ret_3244 = call i64 @__omni_cyw_priv_ip_eq(i64 %t_3242, i64 %t64_3243, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %cond_3245 = icmp ne i64 %fn_ret_3244, 0
  br label %sc_and_done_3233

sc_and_done_3233:
  br label %sc_and_merge_3233

sc_and_merge_3233:
  %and_cond_3246 = phi i1 [ false, %sc_and_short_3233 ], [ %cond_3245, %sc_and_done_3233 ]
  br i1 %and_cond_3246, label %if_then_140, label %if_merge_140

if_then_140:
  %t64_3247 = load i64, i64* %param_ctx_3010
  %t64_3248 = load i64, i64* %param_eth_3012
  %t64_3249 = load i64, i64* %var_ip_3074
  %t64_3250 = load i64, i64* %var_ihl_3083
  %t64_3251 = load i64, i64* %var_tot_len_3087
  %fn_ret_3252 = call i64 @__omni_cyw_priv_tcp_ws_rx(i64 %t64_3247, i64 %t64_3248, i64 %t64_3249, i64 %t64_3250, i64 %t64_3251, i64 -581005069656919554)
  ret i64 %fn_ret_3252
if_merge_140:
  ret i64 0
}


define void @__omni_trampoline_eth_rx_dispatch(i8* %raw_arg) {
entry:
  %raw_int = ptrtoint i8* %raw_arg to i64
  %res = call i64 @__omni_cyw_priv_eth_rx_dispatch(i64 %raw_int, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  ret void
}

define void @__omni_trampoline_pack_eth_rx_dispatch(i8* %raw_pack) {
entry:
  %p64 = bitcast i8* %raw_pack to i64*
  %p0 = getelementptr inbounds i64, i64* %p64, i64 0
  %a0 = load i64, i64* %p0
  %p1 = getelementptr inbounds i64, i64* %p64, i64 1
  %a1 = load i64, i64* %p1
  %p2 = getelementptr inbounds i64, i64* %p64, i64 2
  %a2 = load i64, i64* %p2
  %p3 = getelementptr inbounds i64, i64* %p64, i64 3
  %a3 = load i64, i64* %p3
  %p4 = getelementptr inbounds i64, i64* %p64, i64 4
  %a4 = load i64, i64* %p4
  %p5 = getelementptr inbounds i64, i64* %p64, i64 5
  %a5 = load i64, i64* %p5
  call void @free(i8* %raw_pack)
  %res = call i64 @__omni_cyw_priv_eth_rx_dispatch(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5)
  ret void
}

define i64 @__omni_fn_cyw_poll_one_pkt(i64 %arg0, i64 %arg1, i64 %arg2, i64 %arg3, i64 %arg4, i64 %arg5) {
entry:
  %param_ctx_3254 = alloca i64
  %param_want_reqid_3256 = alloca i64
  %param_ioctl_out_3258 = alloca i64
  %param_ioctl_max_3260 = alloca i64
  %var_st_3262 = alloca i64
  %var_plen_3273 = alloca i64
  %var_ir_3278 = alloca i64
  %var_aligned_3286 = alloca i64
  %var_rx_3289 = alloca i64
  %var_read_n_3291 = alloca i64
  %var_cmd_3296 = alloca i64
  %var_extra_3309 = alloca i64
  %var_dump_3313 = alloca i64
  %var_step_3317 = alloca i64
  %var_s_len_3330 = alloca i64
  %var_s_not_3334 = alloca i64
  %var_chan_3352 = alloca i64
  %var_hdrlen_3356 = alloca i64
  %var_io_3373 = alloca i64
  %var_flags_3377 = alloca i64
  %var_status_3381 = alloca i64
  %var_rx_id_3385 = alloca i64
  %var_pay_3395 = alloca i64
  %var_avail_3400 = alloca i64
  %var_cp_3412 = alloca i64
  %var_bdc_3437 = alloca i64
  %var_doff_3441 = alloca i64
  %var_eth_3447 = alloca i64
  %var_eth_len_3453 = alloca i64
  %var_ev_flags_3459 = alloca i64
  %var_ev_type_3463 = alloca i64
  %var_ev_status_3467 = alloca i64
  %var_jf_3471 = alloca i64
  %var_bdc_3516 = alloca i64
  %var_doff_3520 = alloca i64
  %var_eth_3526 = alloca i64
  %var_eth_len_3532 = alloca i64
  %param_unset_ctx_3253 = icmp eq i64 %arg0, -581005069656919554
  %param_dsel_ctx_3253 = select i1 %param_unset_ctx_3253, i64 0, i64 %arg0
  store i64 %param_dsel_ctx_3253, i64* %param_ctx_3254
  %param_unset_want_reqid_3255 = icmp eq i64 %arg1, -581005069656919554
  %param_dsel_want_reqid_3255 = select i1 %param_unset_want_reqid_3255, i64 0, i64 %arg1
  store i64 %param_dsel_want_reqid_3255, i64* %param_want_reqid_3256
  %param_unset_ioctl_out_3257 = icmp eq i64 %arg2, -581005069656919554
  %param_dsel_ioctl_out_3257 = select i1 %param_unset_ioctl_out_3257, i64 0, i64 %arg2
  store i64 %param_dsel_ioctl_out_3257, i64* %param_ioctl_out_3258
  %param_unset_ioctl_max_3259 = icmp eq i64 %arg3, -581005069656919554
  %param_dsel_ioctl_max_3259 = select i1 %param_unset_ioctl_max_3259, i64 0, i64 %arg3
  store i64 %param_dsel_ioctl_max_3259, i64* %param_ioctl_max_3260
  call void @__omni_c_return_ptr(i8* null)
  %ext_i64_3261 = call i64 @cyw_ll_read_reg(i64 0, i64 8, i64 4)
  store i64 %ext_i64_3261, i64* %var_st_3262
  %t64_3264 = load i64, i64* %var_st_3262
  %cond_3265 = icmp eq i64 %t64_3264, 4294967295
  br i1 %cond_3265, label %sc_or_short_3263, label %sc_or_rhs_3263

sc_or_short_3263:
  br label %sc_or_merge_3263

sc_or_rhs_3263:
  %t64_3266 = load i64, i64* %var_st_3262
  %t_3267 = and i64 %t64_3266, 256
  %cond_3268 = icmp eq i64 %t_3267, 0
  br label %sc_or_done_3263

sc_or_done_3263:
  br label %sc_or_merge_3263

sc_or_merge_3263:
  %or_cond_3269 = phi i1 [ true, %sc_or_short_3263 ], [ %cond_3268, %sc_or_done_3263 ]
  br i1 %or_cond_3269, label %if_then_141, label %if_merge_141

if_then_141:
  ret i64 0
if_merge_141:
  call void @__omni_c_return_ptr(i8* null)
  %t64_3270 = load i64, i64* %var_st_3262
  %t_3271 = ashr i64 %t64_3270, 9
  %t_3272 = and i64 %t_3271, 2047
  store i64 %t_3272, i64* %var_plen_3273
  %t64_3274 = load i64, i64* %var_plen_3273
  %cond_3275 = icmp eq i64 %t64_3274, 0
  br i1 %cond_3275, label %if_then_142, label %if_merge_142

if_then_142:
  %ext_i64_3276 = call i64 @cyw_ll_write_reg(i64 1, i64 65549, i64 1, i64 1)
  call void @__omni_c_return_ptr(i8* null)
  %ext_i64_3277 = call i64 @cyw_ll_read_reg(i64 0, i64 4, i64 2)
  store i64 %ext_i64_3277, i64* %var_ir_3278
  %t64_3279 = load i64, i64* %var_ir_3278
  %cond_3280 = icmp ne i64 %t64_3279, 0
  br i1 %cond_3280, label %if_then_143, label %if_merge_143

if_then_143:
  %t64_3281 = load i64, i64* %var_ir_3278
  %ext_i64_3282 = call i64 @cyw_ll_write_reg(i64 0, i64 4, i64 %t64_3281, i64 2)
  br label %if_merge_143

if_merge_143:
  ret i64 0
if_merge_142:
  call void @__omni_c_return_ptr(i8* null)
  %t64_3283 = load i64, i64* %var_plen_3273
  %t_3284 = add i64 %t64_3283, 3
  %t_3285 = and i64 %t_3284, 2044
  store i64 %t_3285, i64* %var_aligned_3286
  call void @__omni_c_return_ptr(i8* null)
  %t64_3287 = load i64, i64* %param_ctx_3254
  %t_3288 = add i64 %t64_3287, 2880
  store i64 %t_3288, i64* %var_rx_3289
  call void @__omni_c_return_ptr(i8* null)
  %t64_3290 = load i64, i64* %var_aligned_3286
  store i64 %t64_3290, i64* %var_read_n_3291
  %t64_3292 = load i64, i64* %var_read_n_3291
  %cond_3293 = icmp sgt i64 %t64_3292, 1760
  br i1 %cond_3293, label %if_then_144, label %if_merge_144

if_then_144:
  call void @__omni_c_return_ptr(i8* null)
  store i64 1760, i64* %var_read_n_3291
  br label %if_merge_144

if_merge_144:
  call void @__omni_c_return_ptr(i8* null)
  %t64_3294 = load i64, i64* %var_aligned_3286
  %ext_i64_3295 = call i64 @cyw_ll_pack_cmd(i64 0, i64 1, i64 2, i64 0, i64 %t64_3294)
  store i64 %ext_i64_3295, i64* %var_cmd_3296
  %ext_i64_3297 = call i64 @gpio_put(i64 25, i64 0)
  %t64_3298 = load i64, i64* %var_cmd_3296
  %ext_i64_3299 = call i64 @cyw_ll_gspi_write_u32(i64 %t64_3298)
  %t64_3300 = load i64, i64* %var_rx_3289
  %t64_3301 = load i64, i64* %var_read_n_3291
  %ext_i64_3302 = call i64 @cyw_ll_gspi_read_bytes(i64 %t64_3300, i64 %t64_3301)
  %t64_3303 = load i64, i64* %var_aligned_3286
  %t64_3304 = load i64, i64* %var_read_n_3291
  %cond_3305 = icmp sgt i64 %t64_3303, %t64_3304
  br i1 %cond_3305, label %if_then_145, label %if_merge_145

if_then_145:
  call void @__omni_c_return_ptr(i8* null)
  %t64_3306 = load i64, i64* %var_aligned_3286
  %t64_3307 = load i64, i64* %var_read_n_3291
  %t_3308 = sub i64 %t64_3306, %t64_3307
  store i64 %t_3308, i64* %var_extra_3309
  call void @__omni_c_return_ptr(i8* null)
  %t64_3310 = load i64, i64* %param_ctx_3254
  %t_3311 = add i64 %t64_3310, 8000
  %t_3312 = add i64 %t_3311, 400
  store i64 %t_3312, i64* %var_dump_3313
  br label %while_cond_146

while_cond_146:
  %t64_3314 = load i64, i64* %var_extra_3309
  %cond_3315 = icmp sgt i64 %t64_3314, 0
  br i1 %cond_3315, label %while_body_146, label %while_exit_146

while_body_146:
  call void @__omni_c_return_ptr(i8* null)
  %t64_3316 = load i64, i64* %var_extra_3309
  store i64 %t64_3316, i64* %var_step_3317
  %t64_3318 = load i64, i64* %var_step_3317
  %cond_3319 = icmp sgt i64 %t64_3318, 64
  br i1 %cond_3319, label %if_then_147, label %if_merge_147

if_then_147:
  call void @__omni_c_return_ptr(i8* null)
  store i64 64, i64* %var_step_3317
  br label %if_merge_147

if_merge_147:
  %t64_3320 = load i64, i64* %var_dump_3313
  %t64_3321 = load i64, i64* %var_step_3317
  %ext_i64_3322 = call i64 @cyw_ll_gspi_read_bytes(i64 %t64_3320, i64 %t64_3321)
  call void @__omni_c_return_ptr(i8* null)
  %t64_3323 = load i64, i64* %var_extra_3309
  %t64_3324 = load i64, i64* %var_step_3317
  %t_3325 = sub i64 %t64_3323, %t64_3324
  store i64 %t_3325, i64* %var_extra_3309
  br label %while_cond_146

while_exit_146:
  br label %if_merge_145

if_merge_145:
  %ext_i64_3326 = call i64 @gpio_put(i64 25, i64 1)
  call void @__omni_c_return_ptr(i8* null)
  %t64_3327 = load i64, i64* %var_rx_3289
  %t_3328 = add i64 %t64_3327, 0
  %fn_ret_3329 = call i64 @__omni_cyw_priv_r16_le(i64 %t_3328, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  store i64 %fn_ret_3329, i64* %var_s_len_3330
  call void @__omni_c_return_ptr(i8* null)
  %t64_3331 = load i64, i64* %var_rx_3289
  %t_3332 = add i64 %t64_3331, 2
  %fn_ret_3333 = call i64 @__omni_cyw_priv_r16_le(i64 %t_3332, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  store i64 %fn_ret_3333, i64* %var_s_not_3334
  %t64_3337 = load i64, i64* %var_s_len_3330
  %t64_3338 = load i64, i64* %var_s_not_3334
  %t_3339 = xor i64 %t64_3337, %t64_3338
  %cond_3340 = icmp ne i64 %t_3339, 65535
  br i1 %cond_3340, label %sc_or_short_3336, label %sc_or_rhs_3336

sc_or_short_3336:
  br label %sc_or_merge_3336

sc_or_rhs_3336:
  %t64_3341 = load i64, i64* %var_s_len_3330
  %cond_3342 = icmp slt i64 %t64_3341, 12
  br label %sc_or_done_3336

sc_or_done_3336:
  br label %sc_or_merge_3336

sc_or_merge_3336:
  %or_cond_3343 = phi i1 [ true, %sc_or_short_3336 ], [ %cond_3342, %sc_or_done_3336 ]
  br i1 %or_cond_3343, label %sc_or_short_3335, label %sc_or_rhs_3335

sc_or_short_3335:
  br label %sc_or_merge_3335

sc_or_rhs_3335:
  %t64_3344 = load i64, i64* %var_s_len_3330
  %t64_3345 = load i64, i64* %var_read_n_3291
  %cond_3346 = icmp sgt i64 %t64_3344, %t64_3345
  br label %sc_or_done_3335

sc_or_done_3335:
  br label %sc_or_merge_3335

sc_or_merge_3335:
  %or_cond_3347 = phi i1 [ true, %sc_or_short_3335 ], [ %cond_3346, %sc_or_done_3335 ]
  br i1 %or_cond_3347, label %if_then_148, label %if_merge_148

if_then_148:
  ret i64 1
if_merge_148:
  call void @__omni_c_return_ptr(i8* null)
  %t64_3348 = load i64, i64* %var_rx_3289
  %t_3349 = add i64 %t64_3348, 5
  %fn_ret_3350 = call i64 @__omni_cyw_priv_r8(i64 %t_3349, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t_3351 = and i64 %fn_ret_3350, 15
  store i64 %t_3351, i64* %var_chan_3352
  call void @__omni_c_return_ptr(i8* null)
  %t64_3353 = load i64, i64* %var_rx_3289
  %t_3354 = add i64 %t64_3353, 7
  %fn_ret_3355 = call i64 @__omni_cyw_priv_r8(i64 %t_3354, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  store i64 %fn_ret_3355, i64* %var_hdrlen_3356
  %t64_3358 = load i64, i64* %var_hdrlen_3356
  %cond_3359 = icmp slt i64 %t64_3358, 12
  br i1 %cond_3359, label %sc_or_short_3357, label %sc_or_rhs_3357

sc_or_short_3357:
  br label %sc_or_merge_3357

sc_or_rhs_3357:
  %t64_3360 = load i64, i64* %var_hdrlen_3356
  %t64_3361 = load i64, i64* %var_s_len_3330
  %cond_3362 = icmp sgt i64 %t64_3360, %t64_3361
  br label %sc_or_done_3357

sc_or_done_3357:
  br label %sc_or_merge_3357

sc_or_merge_3357:
  %or_cond_3363 = phi i1 [ true, %sc_or_short_3357 ], [ %cond_3362, %sc_or_done_3357 ]
  br i1 %or_cond_3363, label %if_then_149, label %if_merge_149

if_then_149:
  ret i64 1
if_merge_149:
  %t64_3364 = load i64, i64* %var_chan_3352
  %cond_3365 = icmp eq i64 %t64_3364, 0
  br i1 %cond_3365, label %if_then_150, label %if_merge_150

if_then_150:
  %t64_3366 = load i64, i64* %var_hdrlen_3356
  %t_3367 = add i64 %t64_3366, 16
  %t64_3368 = load i64, i64* %var_s_len_3330
  %cond_3369 = icmp sgt i64 %t_3367, %t64_3368
  br i1 %cond_3369, label %if_then_151, label %if_merge_151

if_then_151:
  ret i64 1
if_merge_151:
  call void @__omni_c_return_ptr(i8* null)
  %t64_3370 = load i64, i64* %var_rx_3289
  %t64_3371 = load i64, i64* %var_hdrlen_3356
  %t_3372 = add i64 %t64_3370, %t64_3371
  store i64 %t_3372, i64* %var_io_3373
  call void @__omni_c_return_ptr(i8* null)
  %t64_3374 = load i64, i64* %var_io_3373
  %t_3375 = add i64 %t64_3374, 8
  %fn_ret_3376 = call i64 @__omni_cyw_priv_r32_le(i64 %t_3375, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  store i64 %fn_ret_3376, i64* %var_flags_3377
  call void @__omni_c_return_ptr(i8* null)
  %t64_3378 = load i64, i64* %var_io_3373
  %t_3379 = add i64 %t64_3378, 12
  %fn_ret_3380 = call i64 @__omni_cyw_priv_r32_le(i64 %t_3379, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  store i64 %fn_ret_3380, i64* %var_status_3381
  call void @__omni_c_return_ptr(i8* null)
  %t64_3382 = load i64, i64* %var_flags_3377
  %t_3383 = ashr i64 %t64_3382, 16
  %t_3384 = and i64 %t_3383, 65535
  store i64 %t_3384, i64* %var_rx_id_3385
  %t64_3387 = load i64, i64* %param_want_reqid_3256
  %cond_3388 = icmp ne i64 %t64_3387, 0
  br i1 %cond_3388, label %sc_and_rhs_3386, label %sc_and_short_3386

sc_and_short_3386:
  br label %sc_and_merge_3386

sc_and_rhs_3386:
  %t64_3389 = load i64, i64* %var_rx_id_3385
  %t64_3390 = load i64, i64* %param_want_reqid_3256
  %cond_3391 = icmp eq i64 %t64_3389, %t64_3390
  br label %sc_and_done_3386

sc_and_done_3386:
  br label %sc_and_merge_3386

sc_and_merge_3386:
  %and_cond_3392 = phi i1 [ false, %sc_and_short_3386 ], [ %cond_3391, %sc_and_done_3386 ]
  br i1 %and_cond_3392, label %if_then_152, label %if_merge_152

if_then_152:
  call void @__omni_c_return_ptr(i8* null)
  %t64_3393 = load i64, i64* %var_io_3373
  %t_3394 = add i64 %t64_3393, 16
  store i64 %t_3394, i64* %var_pay_3395
  call void @__omni_c_return_ptr(i8* null)
  %t64_3396 = load i64, i64* %var_s_len_3330
  %t64_3397 = load i64, i64* %var_hdrlen_3356
  %t_3398 = add i64 %t64_3397, 16
  %t_3399 = sub i64 %t64_3396, %t_3398
  store i64 %t_3399, i64* %var_avail_3400
  %t64_3403 = load i64, i64* %param_ioctl_out_3258
  %cond_3404 = icmp ne i64 %t64_3403, 0
  br i1 %cond_3404, label %sc_and_rhs_3402, label %sc_and_short_3402

sc_and_short_3402:
  br label %sc_and_merge_3402

sc_and_rhs_3402:
  %t64_3405 = load i64, i64* %param_ioctl_max_3260
  %cond_3406 = icmp sgt i64 %t64_3405, 0
  br label %sc_and_done_3402

sc_and_done_3402:
  br label %sc_and_merge_3402

sc_and_merge_3402:
  %and_cond_3407 = phi i1 [ false, %sc_and_short_3402 ], [ %cond_3406, %sc_and_done_3402 ]
  br i1 %and_cond_3407, label %sc_and_rhs_3401, label %sc_and_short_3401

sc_and_short_3401:
  br label %sc_and_merge_3401

sc_and_rhs_3401:
  %t64_3408 = load i64, i64* %var_avail_3400
  %cond_3409 = icmp sgt i64 %t64_3408, 0
  br label %sc_and_done_3401

sc_and_done_3401:
  br label %sc_and_merge_3401

sc_and_merge_3401:
  %and_cond_3410 = phi i1 [ false, %sc_and_short_3401 ], [ %cond_3409, %sc_and_done_3401 ]
  br i1 %and_cond_3410, label %if_then_153, label %if_merge_153

if_then_153:
  call void @__omni_c_return_ptr(i8* null)
  %t64_3411 = load i64, i64* %var_avail_3400
  store i64 %t64_3411, i64* %var_cp_3412
  %t64_3413 = load i64, i64* %var_cp_3412
  %t64_3414 = load i64, i64* %param_ioctl_max_3260
  %cond_3415 = icmp sgt i64 %t64_3413, %t64_3414
  br i1 %cond_3415, label %if_then_154, label %if_merge_154

if_then_154:
  call void @__omni_c_return_ptr(i8* null)
  %t64_3416 = load i64, i64* %param_ioctl_max_3260
  store i64 %t64_3416, i64* %var_cp_3412
  br label %if_merge_154

if_merge_154:
  %t64_3417 = load i64, i64* %param_ioctl_out_3258
  %t64_3418 = load i64, i64* %var_pay_3395
  %t64_3419 = load i64, i64* %var_cp_3412
  %fn_ret_3420 = call i64 @__omni_cyw_priv_bcopy(i64 %t64_3417, i64 %t64_3418, i64 %t64_3419, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  br label %if_merge_153

if_merge_153:
  %t64_3422 = load i64, i64* %var_status_3381
  %cond_3423 = icmp eq i64 %t64_3422, 0
  br i1 %cond_3423, label %sc_and_rhs_3421, label %sc_and_short_3421

sc_and_short_3421:
  br label %sc_and_merge_3421

sc_and_rhs_3421:
  %t64_3424 = load i64, i64* %var_flags_3377
  %t_3425 = and i64 %t64_3424, 1
  %cond_3426 = icmp eq i64 %t_3425, 0
  br label %sc_and_done_3421

sc_and_done_3421:
  br label %sc_and_merge_3421

sc_and_merge_3421:
  %and_cond_3427 = phi i1 [ false, %sc_and_short_3421 ], [ %cond_3426, %sc_and_done_3421 ]
  br i1 %and_cond_3427, label %if_then_155, label %if_merge_155

if_then_155:
  ret i64 100
if_merge_155:
  ret i64 101
if_merge_152:
  ret i64 1
if_merge_150:
  %t64_3428 = load i64, i64* %var_chan_3352
  %cond_3429 = icmp eq i64 %t64_3428, 1
  br i1 %cond_3429, label %if_then_156, label %if_merge_156

if_then_156:
  %t64_3430 = load i64, i64* %var_hdrlen_3356
  %t_3431 = add i64 %t64_3430, 4
  %t64_3432 = load i64, i64* %var_s_len_3330
  %cond_3433 = icmp sgt i64 %t_3431, %t64_3432
  br i1 %cond_3433, label %if_then_157, label %if_merge_157

if_then_157:
  ret i64 2
if_merge_157:
  call void @__omni_c_return_ptr(i8* null)
  %t64_3434 = load i64, i64* %var_rx_3289
  %t64_3435 = load i64, i64* %var_hdrlen_3356
  %t_3436 = add i64 %t64_3434, %t64_3435
  store i64 %t_3436, i64* %var_bdc_3437
  call void @__omni_c_return_ptr(i8* null)
  %t64_3438 = load i64, i64* %var_bdc_3437
  %t_3439 = add i64 %t64_3438, 3
  %fn_ret_3440 = call i64 @__omni_cyw_priv_r8(i64 %t_3439, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  store i64 %fn_ret_3440, i64* %var_doff_3441
  call void @__omni_c_return_ptr(i8* null)
  %t64_3442 = load i64, i64* %var_bdc_3437
  %t_3443 = add i64 %t64_3442, 4
  %t64_3444 = load i64, i64* %var_doff_3441
  %t_3445 = mul i64 %t64_3444, 4
  %t_3446 = add i64 %t_3443, %t_3445
  store i64 %t_3446, i64* %var_eth_3447
  call void @__omni_c_return_ptr(i8* null)
  %t64_3448 = load i64, i64* %var_rx_3289
  %t64_3449 = load i64, i64* %var_s_len_3330
  %t_3450 = add i64 %t64_3448, %t64_3449
  %t64_3451 = load i64, i64* %var_eth_3447
  %t_3452 = sub i64 %t_3450, %t64_3451
  store i64 %t_3452, i64* %var_eth_len_3453
  %t64_3454 = load i64, i64* %var_eth_len_3453
  %cond_3455 = icmp sge i64 %t64_3454, 40
  br i1 %cond_3455, label %if_then_158, label %if_merge_158

if_then_158:
  call void @__omni_c_return_ptr(i8* null)
  %t64_3456 = load i64, i64* %var_eth_3447
  %t_3457 = add i64 %t64_3456, 26
  %fn_ret_3458 = call i64 @__omni_cyw_priv_r16_be(i64 %t_3457, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  store i64 %fn_ret_3458, i64* %var_ev_flags_3459
  call void @__omni_c_return_ptr(i8* null)
  %t64_3460 = load i64, i64* %var_eth_3447
  %t_3461 = add i64 %t64_3460, 28
  %fn_ret_3462 = call i64 @__omni_cyw_priv_r32_be(i64 %t_3461, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  store i64 %fn_ret_3462, i64* %var_ev_type_3463
  call void @__omni_c_return_ptr(i8* null)
  %t64_3464 = load i64, i64* %var_eth_3447
  %t_3465 = add i64 %t64_3464, 32
  %fn_ret_3466 = call i64 @__omni_cyw_priv_r32_be(i64 %t_3465, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  store i64 %fn_ret_3466, i64* %var_ev_status_3467
  call void @__omni_c_return_ptr(i8* null)
  %t64_3468 = load i64, i64* %param_ctx_3254
  %t_3469 = add i64 %t64_3468, 4
  %ext_i64_3470 = call i64 @mmio_read32(i64 %t_3469)
  store i64 %ext_i64_3470, i64* %var_jf_3471
  %t64_3473 = load i64, i64* %var_ev_type_3463
  %cond_3474 = icmp eq i64 %t64_3473, 16
  br i1 %cond_3474, label %sc_and_rhs_3472, label %sc_and_short_3472

sc_and_short_3472:
  br label %sc_and_merge_3472

sc_and_rhs_3472:
  %t64_3475 = load i64, i64* %var_ev_status_3467
  %cond_3476 = icmp eq i64 %t64_3475, 0
  br label %sc_and_done_3472

sc_and_done_3472:
  br label %sc_and_merge_3472

sc_and_merge_3472:
  %and_cond_3477 = phi i1 [ false, %sc_and_short_3472 ], [ %cond_3476, %sc_and_done_3472 ]
  br i1 %and_cond_3477, label %if_then_159, label %if_else_159

if_then_159:
  %t64_3478 = load i64, i64* %var_ev_flags_3459
  %t_3479 = and i64 %t64_3478, 1
  %cond_3480 = icmp ne i64 %t_3479, 0
  br i1 %cond_3480, label %if_then_160, label %if_else_160

if_then_160:
  call void @__omni_c_return_ptr(i8* null)
  %t64_3481 = load i64, i64* %var_jf_3471
  %t_3482 = or i64 %t64_3481, 1
  store i64 %t_3482, i64* %var_jf_3471
  br label %if_merge_160

if_else_160:
  call void @__omni_c_return_ptr(i8* null)
  %t64_3483 = load i64, i64* %var_jf_3471
  %t_3484 = and i64 %t64_3483, 2
  store i64 %t_3484, i64* %var_jf_3471
  br label %if_merge_160

if_merge_160:
  br label %if_merge_159

if_else_159:
  %t64_3485 = load i64, i64* %var_ev_type_3463
  %cond_3486 = icmp eq i64 %t64_3485, 46
  br i1 %cond_3486, label %if_then_161, label %if_else_161

if_then_161:
  %t64_3487 = load i64, i64* %var_ev_status_3467
  %cond_3488 = icmp eq i64 %t64_3487, 6
  br i1 %cond_3488, label %if_then_162, label %if_merge_162

if_then_162:
  call void @__omni_c_return_ptr(i8* null)
  %t64_3489 = load i64, i64* %var_jf_3471
  %t_3490 = or i64 %t64_3489, 2
  store i64 %t_3490, i64* %var_jf_3471
  br label %if_merge_162

if_merge_162:
  br label %if_merge_161

if_else_161:
  %t64_3492 = load i64, i64* %var_ev_type_3463
  %cond_3493 = icmp eq i64 %t64_3492, 0
  br i1 %cond_3493, label %sc_and_rhs_3491, label %sc_and_short_3491

sc_and_short_3491:
  br label %sc_and_merge_3491

sc_and_rhs_3491:
  %t64_3494 = load i64, i64* %var_ev_status_3467
  %cond_3495 = icmp eq i64 %t64_3494, 3
  br label %sc_and_done_3491

sc_and_done_3491:
  br label %sc_and_merge_3491

sc_and_merge_3491:
  %and_cond_3496 = phi i1 [ false, %sc_and_short_3491 ], [ %cond_3495, %sc_and_done_3491 ]
  br i1 %and_cond_3496, label %if_then_163, label %if_else_163

if_then_163:
  call void @__omni_c_return_ptr(i8* null)
  %t64_3497 = load i64, i64* %var_jf_3471
  %t_3498 = or i64 %t64_3497, 4
  store i64 %t_3498, i64* %var_jf_3471
  br label %if_merge_163

if_else_163:
  %t64_3499 = load i64, i64* %var_ev_type_3463
  %cond_3500 = icmp eq i64 %t64_3499, 12
  br i1 %cond_3500, label %if_then_164, label %if_merge_164

if_then_164:
  call void @__omni_c_return_ptr(i8* null)
  %t64_3501 = load i64, i64* %var_jf_3471
  %t_3502 = and i64 %t64_3501, 4
  store i64 %t_3502, i64* %var_jf_3471
  br label %if_merge_164

if_merge_164:
  br label %if_merge_163

if_merge_163:
  br label %if_merge_161

if_merge_161:
  br label %if_merge_159

if_merge_159:
  %t64_3503 = load i64, i64* %param_ctx_3254
  %t_3504 = add i64 %t64_3503, 4
  %t64_3505 = load i64, i64* %var_jf_3471
  %ext_i64_3506 = call i64 @mmio_write32(i64 %t_3504, i64 %t64_3505)
  br label %if_merge_158

if_merge_158:
  ret i64 2
if_merge_156:
  %t64_3507 = load i64, i64* %var_chan_3352
  %cond_3508 = icmp eq i64 %t64_3507, 2
  br i1 %cond_3508, label %if_then_165, label %if_merge_165

if_then_165:
  %t64_3509 = load i64, i64* %var_hdrlen_3356
  %t_3510 = add i64 %t64_3509, 4
  %t64_3511 = load i64, i64* %var_s_len_3330
  %cond_3512 = icmp sgt i64 %t_3510, %t64_3511
  br i1 %cond_3512, label %if_then_166, label %if_merge_166

if_then_166:
  ret i64 3
if_merge_166:
  call void @__omni_c_return_ptr(i8* null)
  %t64_3513 = load i64, i64* %var_rx_3289
  %t64_3514 = load i64, i64* %var_hdrlen_3356
  %t_3515 = add i64 %t64_3513, %t64_3514
  store i64 %t_3515, i64* %var_bdc_3516
  call void @__omni_c_return_ptr(i8* null)
  %t64_3517 = load i64, i64* %var_bdc_3516
  %t_3518 = add i64 %t64_3517, 3
  %fn_ret_3519 = call i64 @__omni_cyw_priv_r8(i64 %t_3518, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  store i64 %fn_ret_3519, i64* %var_doff_3520
  call void @__omni_c_return_ptr(i8* null)
  %t64_3521 = load i64, i64* %var_bdc_3516
  %t_3522 = add i64 %t64_3521, 4
  %t64_3523 = load i64, i64* %var_doff_3520
  %t_3524 = mul i64 %t64_3523, 4
  %t_3525 = add i64 %t_3522, %t_3524
  store i64 %t_3525, i64* %var_eth_3526
  call void @__omni_c_return_ptr(i8* null)
  %t64_3527 = load i64, i64* %var_rx_3289
  %t64_3528 = load i64, i64* %var_s_len_3330
  %t_3529 = add i64 %t64_3527, %t64_3528
  %t64_3530 = load i64, i64* %var_eth_3526
  %t_3531 = sub i64 %t_3529, %t64_3530
  store i64 %t_3531, i64* %var_eth_len_3532
  %t64_3533 = load i64, i64* %var_eth_len_3532
  %cond_3534 = icmp sge i64 %t64_3533, 14
  br i1 %cond_3534, label %if_then_167, label %if_merge_167

if_then_167:
  %t64_3535 = load i64, i64* %param_ctx_3254
  %t64_3536 = load i64, i64* %var_eth_3526
  %t64_3537 = load i64, i64* %var_eth_len_3532
  %fn_ret_3538 = call i64 @__omni_cyw_priv_eth_rx_dispatch(i64 %t64_3535, i64 %t64_3536, i64 %t64_3537, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  br label %if_merge_167

if_merge_167:
  ret i64 3
if_merge_165:
  ret i64 1
}


define void @__omni_trampoline_cyw_poll_one_pkt(i8* %raw_arg) {
entry:
  %raw_int = ptrtoint i8* %raw_arg to i64
  %res = call i64 @__omni_fn_cyw_poll_one_pkt(i64 %raw_int, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  ret void
}

define void @__omni_trampoline_pack_cyw_poll_one_pkt(i8* %raw_pack) {
entry:
  %p64 = bitcast i8* %raw_pack to i64*
  %p0 = getelementptr inbounds i64, i64* %p64, i64 0
  %a0 = load i64, i64* %p0
  %p1 = getelementptr inbounds i64, i64* %p64, i64 1
  %a1 = load i64, i64* %p1
  %p2 = getelementptr inbounds i64, i64* %p64, i64 2
  %a2 = load i64, i64* %p2
  %p3 = getelementptr inbounds i64, i64* %p64, i64 3
  %a3 = load i64, i64* %p3
  %p4 = getelementptr inbounds i64, i64* %p64, i64 4
  %a4 = load i64, i64* %p4
  %p5 = getelementptr inbounds i64, i64* %p64, i64 5
  %a5 = load i64, i64* %p5
  call void @free(i8* %raw_pack)
  %res = call i64 @__omni_fn_cyw_poll_one_pkt(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5)
  ret void
}

define i64 @__omni_fn_cyw_do_ioctl(i64 %arg0, i64 %arg1, i64 %arg2, i64 %arg3, i64 %arg4, i64 %arg5) {
entry:
  %param_ctx_3540 = alloca i64
  %param_is_set_3542 = alloca i64
  %param_cmd_3544 = alloca i64
  %param_data_ptr_3546 = alloca i64
  %param_data_len_3548 = alloca i64
  %var_tx_3551 = alloca i64
  %var_outlen_3555 = alloca i64
  %var_txlen_3558 = alloca i64
  %var_reqid_3560 = alloca i64
  %var_flg_3598 = alloca i64
  %var_waited_3624 = alloca i64
  %var_rc_3632 = alloca i64
  %param_unset_ctx_3539 = icmp eq i64 %arg0, -581005069656919554
  %param_dsel_ctx_3539 = select i1 %param_unset_ctx_3539, i64 0, i64 %arg0
  store i64 %param_dsel_ctx_3539, i64* %param_ctx_3540
  %param_unset_is_set_3541 = icmp eq i64 %arg1, -581005069656919554
  %param_dsel_is_set_3541 = select i1 %param_unset_is_set_3541, i64 0, i64 %arg1
  store i64 %param_dsel_is_set_3541, i64* %param_is_set_3542
  %param_unset_cmd_3543 = icmp eq i64 %arg2, -581005069656919554
  %param_dsel_cmd_3543 = select i1 %param_unset_cmd_3543, i64 0, i64 %arg2
  store i64 %param_dsel_cmd_3543, i64* %param_cmd_3544
  %param_unset_data_ptr_3545 = icmp eq i64 %arg3, -581005069656919554
  %param_dsel_data_ptr_3545 = select i1 %param_unset_data_ptr_3545, i64 0, i64 %arg3
  store i64 %param_dsel_data_ptr_3545, i64* %param_data_ptr_3546
  %param_unset_data_len_3547 = icmp eq i64 %arg4, -581005069656919554
  %param_dsel_data_len_3547 = select i1 %param_unset_data_len_3547, i64 0, i64 %arg4
  store i64 %param_dsel_data_len_3547, i64* %param_data_len_3548
  call void @__omni_c_return_ptr(i8* null)
  %t64_3549 = load i64, i64* %param_ctx_3540
  %t_3550 = add i64 %t64_3549, 4672
  store i64 %t_3550, i64* %var_tx_3551
  call void @__omni_c_return_ptr(i8* null)
  %t64_3552 = load i64, i64* %param_data_len_3548
  %t_3553 = add i64 %t64_3552, 3
  %t_3554 = and i64 %t_3553, 65532
  store i64 %t_3554, i64* %var_outlen_3555
  call void @__omni_c_return_ptr(i8* null)
  %t64_3556 = load i64, i64* %var_outlen_3555
  %t_3557 = add i64 28, %t64_3556
  store i64 %t_3557, i64* %var_txlen_3558
  call void @__omni_c_return_ptr(i8* null)
  %fn_ret_3559 = call i64 @__omni_cyw_priv_sdpcm_next_reqid(i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  store i64 %fn_ret_3559, i64* %var_reqid_3560
  %t64_3561 = load i64, i64* %var_tx_3551
  %t64_3562 = load i64, i64* %var_outlen_3555
  %t_3563 = add i64 28, %t64_3562
  %fn_ret_3564 = call i64 @__omni_cyw_priv_bzero(i64 %t64_3561, i64 %t_3563, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_3565 = load i64, i64* %var_tx_3551
  %t_3566 = add i64 %t64_3565, 0
  %t64_3567 = load i64, i64* %var_txlen_3558
  %fn_ret_3568 = call i64 @__omni_cyw_priv_w16_le(i64 %t_3566, i64 %t64_3567, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_3569 = load i64, i64* %var_tx_3551
  %t_3570 = add i64 %t64_3569, 2
  %t64_3571 = load i64, i64* %var_txlen_3558
  %t_3572 = xor i64 %t64_3571, 65535
  %t_3573 = and i64 %t_3572, 65535
  %fn_ret_3574 = call i64 @__omni_cyw_priv_w16_le(i64 %t_3570, i64 %t_3573, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_3575 = load i64, i64* %var_tx_3551
  %t_3576 = add i64 %t64_3575, 4
  %fn_ret_3577 = call i64 @__omni_cyw_priv_sdpcm_next_seq(i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %fn_ret_3578 = call i64 @__omni_cyw_priv_w8(i64 %t_3576, i64 %fn_ret_3577, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_3579 = load i64, i64* %var_tx_3551
  %t_3580 = add i64 %t64_3579, 5
  %fn_ret_3581 = call i64 @__omni_cyw_priv_w8(i64 %t_3580, i64 0, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_3582 = load i64, i64* %var_tx_3551
  %t_3583 = add i64 %t64_3582, 7
  %fn_ret_3584 = call i64 @__omni_cyw_priv_w8(i64 %t_3583, i64 12, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_3585 = load i64, i64* %var_tx_3551
  %t_3586 = add i64 %t64_3585, 12
  %t64_3587 = load i64, i64* %param_cmd_3544
  %fn_ret_3588 = call i64 @__omni_cyw_priv_w32_le(i64 %t_3586, i64 %t64_3587, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_3589 = load i64, i64* %var_tx_3551
  %t_3590 = add i64 %t64_3589, 16
  %t64_3591 = load i64, i64* %var_outlen_3555
  %fn_ret_3592 = call i64 @__omni_cyw_priv_w16_le(i64 %t_3590, i64 %t64_3591, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_3593 = load i64, i64* %var_tx_3551
  %t_3594 = add i64 %t64_3593, 18
  %fn_ret_3595 = call i64 @__omni_cyw_priv_w16_le(i64 %t_3594, i64 0, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  call void @__omni_c_return_ptr(i8* null)
  %t64_3596 = load i64, i64* %var_reqid_3560
  %t_3597 = shl i64 %t64_3596, 16
  store i64 %t_3597, i64* %var_flg_3598
  %t64_3599 = load i64, i64* %param_is_set_3542
  %cond_3600 = icmp ne i64 %t64_3599, 0
  br i1 %cond_3600, label %if_then_168, label %if_merge_168

if_then_168:
  call void @__omni_c_return_ptr(i8* null)
  %t64_3601 = load i64, i64* %var_flg_3598
  %t_3602 = or i64 %t64_3601, 2
  store i64 %t_3602, i64* %var_flg_3598
  br label %if_merge_168

if_merge_168:
  %t64_3603 = load i64, i64* %var_tx_3551
  %t_3604 = add i64 %t64_3603, 20
  %t64_3605 = load i64, i64* %var_flg_3598
  %fn_ret_3606 = call i64 @__omni_cyw_priv_w32_le(i64 %t_3604, i64 %t64_3605, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_3607 = load i64, i64* %var_tx_3551
  %t_3608 = add i64 %t64_3607, 24
  %fn_ret_3609 = call i64 @__omni_cyw_priv_w32_le(i64 %t_3608, i64 0, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_3611 = load i64, i64* %param_data_len_3548
  %cond_3612 = icmp sgt i64 %t64_3611, 0
  br i1 %cond_3612, label %sc_and_rhs_3610, label %sc_and_short_3610

sc_and_short_3610:
  br label %sc_and_merge_3610

sc_and_rhs_3610:
  %t64_3613 = load i64, i64* %param_data_ptr_3546
  %cond_3614 = icmp ne i64 %t64_3613, 0
  br label %sc_and_done_3610

sc_and_done_3610:
  br label %sc_and_merge_3610

sc_and_merge_3610:
  %and_cond_3615 = phi i1 [ false, %sc_and_short_3610 ], [ %cond_3614, %sc_and_done_3610 ]
  br i1 %and_cond_3615, label %if_then_169, label %if_merge_169

if_then_169:
  %t64_3616 = load i64, i64* %var_tx_3551
  %t_3617 = add i64 %t64_3616, 28
  %t64_3618 = load i64, i64* %param_data_ptr_3546
  %t64_3619 = load i64, i64* %param_data_len_3548
  %fn_ret_3620 = call i64 @__omni_cyw_priv_bcopy(i64 %t_3617, i64 %t64_3618, i64 %t64_3619, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  br label %if_merge_169

if_merge_169:
  %t64_3621 = load i64, i64* %var_tx_3551
  %t64_3622 = load i64, i64* %var_txlen_3558
  %fn_ret_3623 = call i64 @__omni_fn_cyw_f2_write(i64 %t64_3621, i64 %t64_3622, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  call void @__omni_c_return_ptr(i8* null)
  store i64 0, i64* %var_waited_3624
  br label %while_cond_170

while_cond_170:
  %t64_3625 = load i64, i64* %var_waited_3624
  %cond_3626 = icmp slt i64 %t64_3625, 300
  br i1 %cond_3626, label %while_body_170, label %while_exit_170

while_body_170:
  call void @__omni_c_return_ptr(i8* null)
  %t64_3627 = load i64, i64* %param_ctx_3540
  %t64_3628 = load i64, i64* %var_reqid_3560
  %t64_3629 = load i64, i64* %param_data_ptr_3546
  %t64_3630 = load i64, i64* %param_data_len_3548
  %fn_ret_3631 = call i64 @__omni_fn_cyw_poll_one_pkt(i64 %t64_3627, i64 %t64_3628, i64 %t64_3629, i64 %t64_3630, i64 -581005069656919554, i64 -581005069656919554)
  store i64 %fn_ret_3631, i64* %var_rc_3632
  %t64_3633 = load i64, i64* %var_rc_3632
  %cond_3634 = icmp eq i64 %t64_3633, 100
  br i1 %cond_3634, label %if_then_171, label %if_merge_171

if_then_171:
  ret i64 0
if_merge_171:
  %t64_3635 = load i64, i64* %var_rc_3632
  %cond_3636 = icmp eq i64 %t64_3635, 101
  br i1 %cond_3636, label %if_then_172, label %if_merge_172

if_then_172:
  ret i64 -1
if_merge_172:
  %t64_3637 = load i64, i64* %var_rc_3632
  %cond_3638 = icmp eq i64 %t64_3637, 0
  br i1 %cond_3638, label %if_then_173, label %if_merge_173

if_then_173:
  %ext_i64_3639 = call i64 @sleep_ms(i64 1)
  call void @__omni_c_return_ptr(i8* null)
  %t64_3640 = load i64, i64* %var_waited_3624
  %t_3641 = add i64 %t64_3640, 1
  store i64 %t_3641, i64* %var_waited_3624
  br label %if_merge_173

if_merge_173:
  br label %while_cond_170

while_exit_170:
  ret i64 -2
}


define void @__omni_trampoline_cyw_do_ioctl(i8* %raw_arg) {
entry:
  %raw_int = ptrtoint i8* %raw_arg to i64
  %res = call i64 @__omni_fn_cyw_do_ioctl(i64 %raw_int, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  ret void
}

define void @__omni_trampoline_pack_cyw_do_ioctl(i8* %raw_pack) {
entry:
  %p64 = bitcast i8* %raw_pack to i64*
  %p0 = getelementptr inbounds i64, i64* %p64, i64 0
  %a0 = load i64, i64* %p0
  %p1 = getelementptr inbounds i64, i64* %p64, i64 1
  %a1 = load i64, i64* %p1
  %p2 = getelementptr inbounds i64, i64* %p64, i64 2
  %a2 = load i64, i64* %p2
  %p3 = getelementptr inbounds i64, i64* %p64, i64 3
  %a3 = load i64, i64* %p3
  %p4 = getelementptr inbounds i64, i64* %p64, i64 4
  %a4 = load i64, i64* %p4
  %p5 = getelementptr inbounds i64, i64* %p64, i64 5
  %a5 = load i64, i64* %p5
  call void @free(i8* %raw_pack)
  %res = call i64 @__omni_fn_cyw_do_ioctl(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5)
  ret void
}

define i64 @__omni_cyw_priv_ioctl_set_u32(i64 %arg0, i64 %arg1, i64 %arg2, i64 %arg3, i64 %arg4, i64 %arg5) {
entry:
  %param_ctx_3643 = alloca i64
  %param_cmd_3645 = alloca i64
  %param_val_3647 = alloca i64
  %var_io_3650 = alloca i64
  %param_unset_ctx_3642 = icmp eq i64 %arg0, -581005069656919554
  %param_dsel_ctx_3642 = select i1 %param_unset_ctx_3642, i64 0, i64 %arg0
  store i64 %param_dsel_ctx_3642, i64* %param_ctx_3643
  %param_unset_cmd_3644 = icmp eq i64 %arg1, -581005069656919554
  %param_dsel_cmd_3644 = select i1 %param_unset_cmd_3644, i64 0, i64 %arg1
  store i64 %param_dsel_cmd_3644, i64* %param_cmd_3645
  %param_unset_val_3646 = icmp eq i64 %arg2, -581005069656919554
  %param_dsel_val_3646 = select i1 %param_unset_val_3646, i64 0, i64 %arg2
  store i64 %param_dsel_val_3646, i64* %param_val_3647
  call void @__omni_c_return_ptr(i8* null)
  %t64_3648 = load i64, i64* %param_ctx_3643
  %t_3649 = add i64 %t64_3648, 8000
  store i64 %t_3649, i64* %var_io_3650
  %t64_3651 = load i64, i64* %var_io_3650
  %t64_3652 = load i64, i64* %param_val_3647
  %fn_ret_3653 = call i64 @__omni_cyw_priv_w32_le(i64 %t64_3651, i64 %t64_3652, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_3654 = load i64, i64* %param_ctx_3643
  %t64_3655 = load i64, i64* %param_cmd_3645
  %t64_3656 = load i64, i64* %var_io_3650
  %fn_ret_3657 = call i64 @__omni_fn_cyw_do_ioctl(i64 %t64_3654, i64 1, i64 %t64_3655, i64 %t64_3656, i64 4, i64 -581005069656919554)
  ret i64 %fn_ret_3657
}


define void @__omni_trampoline_ioctl_set_u32(i8* %raw_arg) {
entry:
  %raw_int = ptrtoint i8* %raw_arg to i64
  %res = call i64 @__omni_cyw_priv_ioctl_set_u32(i64 %raw_int, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  ret void
}

define void @__omni_trampoline_pack_ioctl_set_u32(i8* %raw_pack) {
entry:
  %p64 = bitcast i8* %raw_pack to i64*
  %p0 = getelementptr inbounds i64, i64* %p64, i64 0
  %a0 = load i64, i64* %p0
  %p1 = getelementptr inbounds i64, i64* %p64, i64 1
  %a1 = load i64, i64* %p1
  %p2 = getelementptr inbounds i64, i64* %p64, i64 2
  %a2 = load i64, i64* %p2
  %p3 = getelementptr inbounds i64, i64* %p64, i64 3
  %a3 = load i64, i64* %p3
  %p4 = getelementptr inbounds i64, i64* %p64, i64 4
  %a4 = load i64, i64* %p4
  %p5 = getelementptr inbounds i64, i64* %p64, i64 5
  %a5 = load i64, i64* %p5
  call void @free(i8* %raw_pack)
  %res = call i64 @__omni_cyw_priv_ioctl_set_u32(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5)
  ret void
}

define i64 @__omni_cyw_priv_iovar_set_u32(i64 %arg0, i64 %arg1, i64 %arg2, i64 %arg3, i64 %arg4, i64 %arg5) {
entry:
  %param_ctx_3659 = alloca i64
  %param_name_3662 = alloca i8*
  %param_val_3664 = alloca i64
  %var_io_3667 = alloca i64
  %var_nl_3672 = alloca i64
  store i8* null, i8** %param_name_3662
  %param_unset_ctx_3658 = icmp eq i64 %arg0, -581005069656919554
  %param_dsel_ctx_3658 = select i1 %param_unset_ctx_3658, i64 0, i64 %arg0
  store i64 %param_dsel_ctx_3658, i64* %param_ctx_3659
  %param_unset_name_3660 = icmp eq i64 %arg1, -581005069656919554
  %param_dsel_name_3660 = select i1 %param_unset_name_3660, i64 0, i64 %arg1
  %param_strp_name_3661 = inttoptr i64 %param_dsel_name_3660 to i8*
  store i8* %param_strp_name_3661, i8** %param_name_3662
  %param_unset_val_3663 = icmp eq i64 %arg2, -581005069656919554
  %param_dsel_val_3663 = select i1 %param_unset_val_3663, i64 0, i64 %arg2
  store i64 %param_dsel_val_3663, i64* %param_val_3664
  call void @__omni_c_return_ptr(i8* null)
  %t64_3665 = load i64, i64* %param_ctx_3659
  %t_3666 = add i64 %t64_3665, 8000
  store i64 %t_3666, i64* %var_io_3667
  call void @__omni_c_return_ptr(i8* null)
  %t64_3668 = load i64, i64* %var_io_3667
  %ptr_ld_3669 = load i8*, i8** %param_name_3662
  %p2i64_3670 = ptrtoint i8* %ptr_ld_3669 to i64
  %fn_ret_3671 = call i64 @__omni_cyw_priv_put_ascii(i64 %t64_3668, i64 %p2i64_3670, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  store i64 %fn_ret_3671, i64* %var_nl_3672
  %t64_3673 = load i64, i64* %var_io_3667
  %t64_3674 = load i64, i64* %var_nl_3672
  %t_3675 = add i64 %t64_3673, %t64_3674
  %fn_ret_3676 = call i64 @__omni_cyw_priv_w8(i64 %t_3675, i64 0, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_3677 = load i64, i64* %var_io_3667
  %t64_3678 = load i64, i64* %var_nl_3672
  %t_3679 = add i64 %t64_3677, %t64_3678
  %t_3680 = add i64 %t_3679, 1
  %t64_3681 = load i64, i64* %param_val_3664
  %fn_ret_3682 = call i64 @__omni_cyw_priv_w32_le(i64 %t_3680, i64 %t64_3681, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_3683 = load i64, i64* %param_ctx_3659
  %t64_3684 = load i64, i64* %var_io_3667
  %t64_3685 = load i64, i64* %var_nl_3672
  %t_3686 = add i64 %t64_3685, 5
  %fn_ret_3687 = call i64 @__omni_fn_cyw_do_ioctl(i64 %t64_3683, i64 1, i64 263, i64 %t64_3684, i64 %t_3686, i64 -581005069656919554)
  ret i64 %fn_ret_3687
}


define void @__omni_trampoline_iovar_set_u32(i8* %raw_arg) {
entry:
  %raw_int = ptrtoint i8* %raw_arg to i64
  %res = call i64 @__omni_cyw_priv_iovar_set_u32(i64 %raw_int, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  ret void
}

define void @__omni_trampoline_pack_iovar_set_u32(i8* %raw_pack) {
entry:
  %p64 = bitcast i8* %raw_pack to i64*
  %p0 = getelementptr inbounds i64, i64* %p64, i64 0
  %a0 = load i64, i64* %p0
  %p1 = getelementptr inbounds i64, i64* %p64, i64 1
  %a1 = load i64, i64* %p1
  %p2 = getelementptr inbounds i64, i64* %p64, i64 2
  %a2 = load i64, i64* %p2
  %p3 = getelementptr inbounds i64, i64* %p64, i64 3
  %a3 = load i64, i64* %p3
  %p4 = getelementptr inbounds i64, i64* %p64, i64 4
  %a4 = load i64, i64* %p4
  %p5 = getelementptr inbounds i64, i64* %p64, i64 5
  %a5 = load i64, i64* %p5
  call void @free(i8* %raw_pack)
  %res = call i64 @__omni_cyw_priv_iovar_set_u32(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5)
  ret void
}

define i64 @__omni_cyw_priv_iovar_set_u32x2(i64 %arg0, i64 %arg1, i64 %arg2, i64 %arg3, i64 %arg4, i64 %arg5) {
entry:
  %param_ctx_3689 = alloca i64
  %param_name_3692 = alloca i8*
  %param_v0_3694 = alloca i64
  %param_v1_3696 = alloca i64
  %var_io_3699 = alloca i64
  %var_nl_3704 = alloca i64
  store i8* null, i8** %param_name_3692
  %param_unset_ctx_3688 = icmp eq i64 %arg0, -581005069656919554
  %param_dsel_ctx_3688 = select i1 %param_unset_ctx_3688, i64 0, i64 %arg0
  store i64 %param_dsel_ctx_3688, i64* %param_ctx_3689
  %param_unset_name_3690 = icmp eq i64 %arg1, -581005069656919554
  %param_dsel_name_3690 = select i1 %param_unset_name_3690, i64 0, i64 %arg1
  %param_strp_name_3691 = inttoptr i64 %param_dsel_name_3690 to i8*
  store i8* %param_strp_name_3691, i8** %param_name_3692
  %param_unset_v0_3693 = icmp eq i64 %arg2, -581005069656919554
  %param_dsel_v0_3693 = select i1 %param_unset_v0_3693, i64 0, i64 %arg2
  store i64 %param_dsel_v0_3693, i64* %param_v0_3694
  %param_unset_v1_3695 = icmp eq i64 %arg3, -581005069656919554
  %param_dsel_v1_3695 = select i1 %param_unset_v1_3695, i64 0, i64 %arg3
  store i64 %param_dsel_v1_3695, i64* %param_v1_3696
  call void @__omni_c_return_ptr(i8* null)
  %t64_3697 = load i64, i64* %param_ctx_3689
  %t_3698 = add i64 %t64_3697, 8000
  store i64 %t_3698, i64* %var_io_3699
  call void @__omni_c_return_ptr(i8* null)
  %t64_3700 = load i64, i64* %var_io_3699
  %ptr_ld_3701 = load i8*, i8** %param_name_3692
  %p2i64_3702 = ptrtoint i8* %ptr_ld_3701 to i64
  %fn_ret_3703 = call i64 @__omni_cyw_priv_put_ascii(i64 %t64_3700, i64 %p2i64_3702, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  store i64 %fn_ret_3703, i64* %var_nl_3704
  %t64_3705 = load i64, i64* %var_io_3699
  %t64_3706 = load i64, i64* %var_nl_3704
  %t_3707 = add i64 %t64_3705, %t64_3706
  %fn_ret_3708 = call i64 @__omni_cyw_priv_w8(i64 %t_3707, i64 0, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_3709 = load i64, i64* %var_io_3699
  %t64_3710 = load i64, i64* %var_nl_3704
  %t_3711 = add i64 %t64_3709, %t64_3710
  %t_3712 = add i64 %t_3711, 1
  %t64_3713 = load i64, i64* %param_v0_3694
  %fn_ret_3714 = call i64 @__omni_cyw_priv_w32_le(i64 %t_3712, i64 %t64_3713, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_3715 = load i64, i64* %var_io_3699
  %t64_3716 = load i64, i64* %var_nl_3704
  %t_3717 = add i64 %t64_3715, %t64_3716
  %t_3718 = add i64 %t_3717, 5
  %t64_3719 = load i64, i64* %param_v1_3696
  %fn_ret_3720 = call i64 @__omni_cyw_priv_w32_le(i64 %t_3718, i64 %t64_3719, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_3721 = load i64, i64* %param_ctx_3689
  %t64_3722 = load i64, i64* %var_io_3699
  %t64_3723 = load i64, i64* %var_nl_3704
  %t_3724 = add i64 %t64_3723, 9
  %fn_ret_3725 = call i64 @__omni_fn_cyw_do_ioctl(i64 %t64_3721, i64 1, i64 263, i64 %t64_3722, i64 %t_3724, i64 -581005069656919554)
  ret i64 %fn_ret_3725
}


define void @__omni_trampoline_iovar_set_u32x2(i8* %raw_arg) {
entry:
  %raw_int = ptrtoint i8* %raw_arg to i64
  %res = call i64 @__omni_cyw_priv_iovar_set_u32x2(i64 %raw_int, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  ret void
}

define void @__omni_trampoline_pack_iovar_set_u32x2(i8* %raw_pack) {
entry:
  %p64 = bitcast i8* %raw_pack to i64*
  %p0 = getelementptr inbounds i64, i64* %p64, i64 0
  %a0 = load i64, i64* %p0
  %p1 = getelementptr inbounds i64, i64* %p64, i64 1
  %a1 = load i64, i64* %p1
  %p2 = getelementptr inbounds i64, i64* %p64, i64 2
  %a2 = load i64, i64* %p2
  %p3 = getelementptr inbounds i64, i64* %p64, i64 3
  %a3 = load i64, i64* %p3
  %p4 = getelementptr inbounds i64, i64* %p64, i64 4
  %a4 = load i64, i64* %p4
  %p5 = getelementptr inbounds i64, i64* %p64, i64 5
  %a5 = load i64, i64* %p5
  call void @free(i8* %raw_pack)
  %res = call i64 @__omni_cyw_priv_iovar_set_u32x2(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5)
  ret void
}

define i64 @__omni_fn_cyw_net_gpio_put(i64 %arg0, i64 %arg1, i64 %arg2, i64 %arg3, i64 %arg4, i64 %arg5) {
entry:
  %param_pin_3727 = alloca i64
  %param_on_3729 = alloca i64
  %var_ctx_3731 = alloca i64
  %var_p_3734 = alloca i64
  %var_mask_3737 = alloca i64
  %var_vbit_3738 = alloca i64
  %var_scr_3743 = alloca i64
  %var_pk_old_3747 = alloca i64
  %var_out_old_3750 = alloca i64
  %var_en_old_3754 = alloca i64
  %var_out_new_3761 = alloca i64
  %var_en_new_3765 = alloca i64
  %param_unset_pin_3726 = icmp eq i64 %arg0, -581005069656919554
  %param_dsel_pin_3726 = select i1 %param_unset_pin_3726, i64 0, i64 %arg0
  store i64 %param_dsel_pin_3726, i64* %param_pin_3727
  %param_unset_on_3728 = icmp eq i64 %arg1, -581005069656919554
  %param_dsel_on_3728 = select i1 %param_unset_on_3728, i64 0, i64 %arg1
  store i64 %param_dsel_on_3728, i64* %param_on_3729
  call void @__omni_c_return_ptr(i8* null)
  %fn_ret_3730 = call i64 @__omni_cyw_priv_get_net_ctx(i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  store i64 %fn_ret_3730, i64* %var_ctx_3731
  call void @__omni_c_return_ptr(i8* null)
  %t64_3732 = load i64, i64* %param_pin_3727
  %t_3733 = and i64 %t64_3732, 7
  store i64 %t_3733, i64* %var_p_3734
  call void @__omni_c_return_ptr(i8* null)
  %t64_3735 = load i64, i64* %var_p_3734
  %t_3736 = shl i64 1, %t64_3735
  store i64 %t_3736, i64* %var_mask_3737
  call void @__omni_c_return_ptr(i8* null)
  store i64 0, i64* %var_vbit_3738
  %t64_3739 = load i64, i64* %param_on_3729
  %cond_3740 = icmp ne i64 %t64_3739, 0
  br i1 %cond_3740, label %if_then_174, label %if_merge_174

if_then_174:
  call void @__omni_c_return_ptr(i8* null)
  %t64_3741 = load i64, i64* %var_mask_3737
  store i64 %t64_3741, i64* %var_vbit_3738
  br label %if_merge_174

if_merge_174:
  call void @__omni_c_return_ptr(i8* null)
  %ext_i64_3742 = call i64 @cyw_ll_sram_scratch()
  store i64 %ext_i64_3742, i64* %var_scr_3743
  call void @__omni_c_return_ptr(i8* null)
  %t64_3744 = load i64, i64* %var_scr_3743
  %t_3745 = add i64 %t64_3744, 12
  %ext_i64_3746 = call i64 @mmio_read32(i64 %t_3745)
  store i64 %ext_i64_3746, i64* %var_pk_old_3747
  call void @__omni_c_return_ptr(i8* null)
  %t64_3748 = load i64, i64* %var_pk_old_3747
  %t_3749 = and i64 %t64_3748, 255
  store i64 %t_3749, i64* %var_out_old_3750
  call void @__omni_c_return_ptr(i8* null)
  %t64_3751 = load i64, i64* %var_pk_old_3747
  %t_3752 = ashr i64 %t64_3751, 8
  %t_3753 = and i64 %t_3752, 255
  store i64 %t_3753, i64* %var_en_old_3754
  call void @__omni_c_return_ptr(i8* null)
  %t64_3755 = load i64, i64* %var_out_old_3750
  %t64_3756 = load i64, i64* %var_mask_3737
  %t_3757 = xor i64 %t64_3756, 255
  %t_3758 = and i64 %t64_3755, %t_3757
  %t64_3759 = load i64, i64* %var_vbit_3738
  %t_3760 = or i64 %t_3758, %t64_3759
  store i64 %t_3760, i64* %var_out_new_3761
  call void @__omni_c_return_ptr(i8* null)
  %t64_3762 = load i64, i64* %var_en_old_3754
  %t64_3763 = load i64, i64* %var_mask_3737
  %t_3764 = or i64 %t64_3762, %t64_3763
  store i64 %t_3764, i64* %var_en_new_3765
  %t64_3766 = load i64, i64* %var_scr_3743
  %t_3767 = add i64 %t64_3766, 12
  %t64_3768 = load i64, i64* %var_out_new_3761
  %t64_3769 = load i64, i64* %var_en_new_3765
  %t_3770 = shl i64 %t64_3769, 8
  %t_3771 = or i64 %t64_3768, %t_3770
  %ext_i64_3772 = call i64 @mmio_write32(i64 %t_3767, i64 %t_3771)
  %t64_3773 = load i64, i64* %var_ctx_3731
  %str_lit_3774 = getelementptr inbounds [8 x i8], [8 x i8]* @.str.cyw_11, i64 0, i64 0
  call void @__omni_c_return_ptr(i8* %str_lit_3774)
  %p2i64_3775 = ptrtoint i8* %str_lit_3774 to i64
  %t64_3776 = load i64, i64* %var_mask_3737
  %t64_3777 = load i64, i64* %var_vbit_3738
  %fn_ret_3778 = call i64 @__omni_cyw_priv_iovar_set_u32x2(i64 %t64_3773, i64 %p2i64_3775, i64 %t64_3776, i64 %t64_3777, i64 -581005069656919554, i64 -581005069656919554)
  %t64_3779 = load i64, i64* %var_en_new_3765
  %ext_i64_3780 = call i64 @cyw_ll_write_bp(i64 402653288, i64 %t64_3779, i64 4)
  %t64_3781 = load i64, i64* %var_out_new_3761
  %ext_i64_3782 = call i64 @cyw_ll_write_bp(i64 402653284, i64 %t64_3781, i64 4)
  ret i64 0
}


define void @__omni_trampoline_cyw_net_gpio_put(i8* %raw_arg) {
entry:
  %raw_int = ptrtoint i8* %raw_arg to i64
  %res = call i64 @__omni_fn_cyw_net_gpio_put(i64 %raw_int, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  ret void
}

define void @__omni_trampoline_pack_cyw_net_gpio_put(i8* %raw_pack) {
entry:
  %p64 = bitcast i8* %raw_pack to i64*
  %p0 = getelementptr inbounds i64, i64* %p64, i64 0
  %a0 = load i64, i64* %p0
  %p1 = getelementptr inbounds i64, i64* %p64, i64 1
  %a1 = load i64, i64* %p1
  %p2 = getelementptr inbounds i64, i64* %p64, i64 2
  %a2 = load i64, i64* %p2
  %p3 = getelementptr inbounds i64, i64* %p64, i64 3
  %a3 = load i64, i64* %p3
  %p4 = getelementptr inbounds i64, i64* %p64, i64 4
  %a4 = load i64, i64* %p4
  %p5 = getelementptr inbounds i64, i64* %p64, i64 5
  %a5 = load i64, i64* %p5
  call void @free(i8* %raw_pack)
  %res = call i64 @__omni_fn_cyw_net_gpio_put(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5)
  ret void
}

define i64 @__omni_fn_cyw_load_clm(i64 %arg0, i64 %arg1, i64 %arg2, i64 %arg3, i64 %arg4, i64 %arg5) {
entry:
  %param_ctx_3784 = alloca i64
  %var_clm_ptr_3786 = alloca i64
  %var_clm_len_3787 = alloca i64
  %var_off_3788 = alloca i64
  %var_io_3791 = alloca i64
  %var_chunk_3798 = alloca i64
  %var_flag_3799 = alloca i64
  %var_nl_3814 = alloca i64
  %var_send_len_3844 = alloca i64
  %param_unset_ctx_3783 = icmp eq i64 %arg0, -581005069656919554
  %param_dsel_ctx_3783 = select i1 %param_unset_ctx_3783, i64 0, i64 %arg0
  store i64 %param_dsel_ctx_3783, i64* %param_ctx_3784
  call void @__omni_c_return_ptr(i8* null)
  %ext_i64_3785 = call i64 @cyw_ll_clm_ptr()
  store i64 %ext_i64_3785, i64* %var_clm_ptr_3786
  call void @__omni_c_return_ptr(i8* null)
  store i64 984, i64* %var_clm_len_3787
  call void @__omni_c_return_ptr(i8* null)
  store i64 0, i64* %var_off_3788
  call void @__omni_c_return_ptr(i8* null)
  %t64_3789 = load i64, i64* %param_ctx_3784
  %t_3790 = add i64 %t64_3789, 8000
  store i64 %t_3790, i64* %var_io_3791
  br label %while_cond_175

while_cond_175:
  %t64_3792 = load i64, i64* %var_off_3788
  %t64_3793 = load i64, i64* %var_clm_len_3787
  %cond_3794 = icmp slt i64 %t64_3792, %t64_3793
  br i1 %cond_3794, label %while_body_175, label %while_exit_175

while_body_175:
  call void @__omni_c_return_ptr(i8* null)
  %t64_3795 = load i64, i64* %var_clm_len_3787
  %t64_3796 = load i64, i64* %var_off_3788
  %t_3797 = sub i64 %t64_3795, %t64_3796
  store i64 %t_3797, i64* %var_chunk_3798
  call void @__omni_c_return_ptr(i8* null)
  store i64 4096, i64* %var_flag_3799
  %t64_3800 = load i64, i64* %var_off_3788
  %cond_3801 = icmp eq i64 %t64_3800, 0
  br i1 %cond_3801, label %if_then_176, label %if_merge_176

if_then_176:
  call void @__omni_c_return_ptr(i8* null)
  %t64_3802 = load i64, i64* %var_flag_3799
  %t_3803 = or i64 %t64_3802, 2
  store i64 %t_3803, i64* %var_flag_3799
  br label %if_merge_176

if_merge_176:
  %t64_3804 = load i64, i64* %var_chunk_3798
  %cond_3805 = icmp sgt i64 %t64_3804, 512
  br i1 %cond_3805, label %if_then_177, label %if_else_177

if_then_177:
  call void @__omni_c_return_ptr(i8* null)
  store i64 512, i64* %var_chunk_3798
  br label %if_merge_177

if_else_177:
  call void @__omni_c_return_ptr(i8* null)
  %t64_3806 = load i64, i64* %var_flag_3799
  %t_3807 = or i64 %t64_3806, 4
  store i64 %t_3807, i64* %var_flag_3799
  br label %if_merge_177

if_merge_177:
  %t64_3808 = load i64, i64* %var_io_3791
  %fn_ret_3809 = call i64 @__omni_cyw_priv_bzero(i64 %t64_3808, i64 560, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  call void @__omni_c_return_ptr(i8* null)
  %t64_3810 = load i64, i64* %var_io_3791
  %str_lit_3811 = getelementptr inbounds [8 x i8], [8 x i8]* @.str.cyw_12, i64 0, i64 0
  call void @__omni_c_return_ptr(i8* %str_lit_3811)
  %p2i64_3812 = ptrtoint i8* %str_lit_3811 to i64
  %fn_ret_3813 = call i64 @__omni_cyw_priv_put_ascii(i64 %t64_3810, i64 %p2i64_3812, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  store i64 %fn_ret_3813, i64* %var_nl_3814
  %t64_3815 = load i64, i64* %var_io_3791
  %t64_3816 = load i64, i64* %var_nl_3814
  %t_3817 = add i64 %t64_3815, %t64_3816
  %fn_ret_3818 = call i64 @__omni_cyw_priv_w8(i64 %t_3817, i64 0, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_3819 = load i64, i64* %var_io_3791
  %t_3820 = add i64 %t64_3819, 8
  %t64_3821 = load i64, i64* %var_flag_3799
  %fn_ret_3822 = call i64 @__omni_cyw_priv_w16_le(i64 %t_3820, i64 %t64_3821, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_3823 = load i64, i64* %var_io_3791
  %t_3824 = add i64 %t64_3823, 10
  %fn_ret_3825 = call i64 @__omni_cyw_priv_w16_le(i64 %t_3824, i64 2, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_3826 = load i64, i64* %var_io_3791
  %t_3827 = add i64 %t64_3826, 12
  %t64_3828 = load i64, i64* %var_chunk_3798
  %fn_ret_3829 = call i64 @__omni_cyw_priv_w32_le(i64 %t_3827, i64 %t64_3828, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_3830 = load i64, i64* %var_io_3791
  %t_3831 = add i64 %t64_3830, 16
  %fn_ret_3832 = call i64 @__omni_cyw_priv_w32_le(i64 %t_3831, i64 0, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_3833 = load i64, i64* %var_io_3791
  %t_3834 = add i64 %t64_3833, 20
  %t64_3835 = load i64, i64* %var_clm_ptr_3786
  %t64_3836 = load i64, i64* %var_off_3788
  %t_3837 = add i64 %t64_3835, %t64_3836
  %t64_3838 = load i64, i64* %var_chunk_3798
  %fn_ret_3839 = call i64 @__omni_cyw_priv_bcopy(i64 %t_3834, i64 %t_3837, i64 %t64_3838, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  call void @__omni_c_return_ptr(i8* null)
  %t64_3840 = load i64, i64* %var_chunk_3798
  %t_3841 = add i64 20, %t64_3840
  %t_3842 = add i64 %t_3841, 7
  %t_3843 = and i64 %t_3842, 65528
  store i64 %t_3843, i64* %var_send_len_3844
  %t64_3845 = load i64, i64* %param_ctx_3784
  %t64_3846 = load i64, i64* %var_io_3791
  %t64_3847 = load i64, i64* %var_send_len_3844
  %fn_ret_3848 = call i64 @__omni_fn_cyw_do_ioctl(i64 %t64_3845, i64 1, i64 263, i64 %t64_3846, i64 %t64_3847, i64 -581005069656919554)
  call void @__omni_c_return_ptr(i8* null)
  %t64_3849 = load i64, i64* %var_off_3788
  %t64_3850 = load i64, i64* %var_chunk_3798
  %t_3851 = add i64 %t64_3849, %t64_3850
  store i64 %t_3851, i64* %var_off_3788
  br label %while_cond_175

while_exit_175:
  ret i64 0
}


define void @__omni_trampoline_cyw_load_clm(i8* %raw_arg) {
entry:
  %raw_int = ptrtoint i8* %raw_arg to i64
  %res = call i64 @__omni_fn_cyw_load_clm(i64 %raw_int, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  ret void
}

define void @__omni_trampoline_pack_cyw_load_clm(i8* %raw_pack) {
entry:
  %p64 = bitcast i8* %raw_pack to i64*
  %p0 = getelementptr inbounds i64, i64* %p64, i64 0
  %a0 = load i64, i64* %p0
  %p1 = getelementptr inbounds i64, i64* %p64, i64 1
  %a1 = load i64, i64* %p1
  %p2 = getelementptr inbounds i64, i64* %p64, i64 2
  %a2 = load i64, i64* %p2
  %p3 = getelementptr inbounds i64, i64* %p64, i64 3
  %a3 = load i64, i64* %p3
  %p4 = getelementptr inbounds i64, i64* %p64, i64 4
  %a4 = load i64, i64* %p4
  %p5 = getelementptr inbounds i64, i64* %p64, i64 5
  %a5 = load i64, i64* %p5
  call void @free(i8* %raw_pack)
  %res = call i64 @__omni_fn_cyw_load_clm(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5)
  ret void
}

define i64 @__omni_fn_cyw_wifi_connect(i64 %arg0, i64 %arg1, i64 %arg2, i64 %arg3, i64 %arg4, i64 %arg5) {
entry:
  %param_ssid_ptr_3853 = alloca i64
  %param_pass_ptr_3855 = alloca i64
  %var_ctx_3857 = alloca i64
  %var_io_3860 = alloca i64
  %var_rc_3866 = alloca i64
  %var_cl_3878 = alloca i64
  %var_el_3931 = alloca i64
  %var_mi_3939 = alloca i64
  %var_ml_3978 = alloca i64
  %var_ssid_len_4038 = alloca i64
  %var_pass_len_4043 = alloca i64
  %var_attempt_4046 = alloca i64
  %var_joined_4047 = alloca i64
  %var_led_4048 = alloca i64
  %var_wsec_4080 = alloca i64
  %var_mfp_val_4120 = alloca i64
  %var_wait_ms_4151 = alloca i64
  %var_drain_k_4154 = alloca i64
  %var_jf_4165 = alloca i64
  %var_dhcp_ms_4218 = alloca i64
  %var_next_tx_ms_4219 = alloca i64
  %var_dk_4222 = alloca i64
  %var_dst_4233 = alloca i64
  %var_bmac_4277 = alloca i64
  %var_bi_4278 = alloca i64
  %param_unset_ssid_ptr_3852 = icmp eq i64 %arg0, -581005069656919554
  %param_dsel_ssid_ptr_3852 = select i1 %param_unset_ssid_ptr_3852, i64 0, i64 %arg0
  store i64 %param_dsel_ssid_ptr_3852, i64* %param_ssid_ptr_3853
  %param_unset_pass_ptr_3854 = icmp eq i64 %arg1, -581005069656919554
  %param_dsel_pass_ptr_3854 = select i1 %param_unset_pass_ptr_3854, i64 0, i64 %arg1
  store i64 %param_dsel_pass_ptr_3854, i64* %param_pass_ptr_3855
  call void @__omni_c_return_ptr(i8* null)
  %fn_ret_3856 = call i64 @__omni_cyw_priv_get_net_ctx(i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  store i64 %fn_ret_3856, i64* %var_ctx_3857
  call void @__omni_c_return_ptr(i8* null)
  %t64_3858 = load i64, i64* %var_ctx_3857
  %t_3859 = add i64 %t64_3858, 8000
  store i64 %t_3859, i64* %var_io_3860
  %t64_3861 = load i64, i64* %var_ctx_3857
  %t_3862 = add i64 %t64_3861, 0
  %ext_i64_3863 = call i64 @mmio_read32(i64 %t_3862)
  %cond_3864 = icmp eq i64 %ext_i64_3863, 0
  br i1 %cond_3864, label %if_then_178, label %if_merge_178

if_then_178:
  call void @__omni_c_return_ptr(i8* null)
  %ext_i64_3865 = call i64 @cyw43_init()
  store i64 %ext_i64_3865, i64* %var_rc_3866
  %t64_3867 = load i64, i64* %var_rc_3866
  %cond_3868 = icmp ne i64 %t64_3867, 0
  br i1 %cond_3868, label %if_then_179, label %if_merge_179

if_then_179:
  %t64_3869 = load i64, i64* %var_rc_3866
  ret i64 %t64_3869
if_merge_179:
  %t64_3870 = load i64, i64* %var_ctx_3857
  %fn_ret_3871 = call i64 @__omni_fn_cyw_load_clm(i64 %t64_3870, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_3872 = load i64, i64* %var_io_3860
  %fn_ret_3873 = call i64 @__omni_cyw_priv_bzero(i64 %t64_3872, i64 32, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  call void @__omni_c_return_ptr(i8* null)
  %t64_3874 = load i64, i64* %var_io_3860
  %str_lit_3875 = getelementptr inbounds [8 x i8], [8 x i8]* @.str.cyw_13, i64 0, i64 0
  call void @__omni_c_return_ptr(i8* %str_lit_3875)
  %p2i64_3876 = ptrtoint i8* %str_lit_3875 to i64
  %fn_ret_3877 = call i64 @__omni_cyw_priv_put_ascii(i64 %t64_3874, i64 %p2i64_3876, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  store i64 %fn_ret_3877, i64* %var_cl_3878
  %t64_3879 = load i64, i64* %var_io_3860
  %t64_3880 = load i64, i64* %var_cl_3878
  %t_3881 = add i64 %t64_3879, %t64_3880
  %fn_ret_3882 = call i64 @__omni_cyw_priv_w8(i64 %t_3881, i64 0, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_3883 = load i64, i64* %var_io_3860
  %t_3884 = add i64 %t64_3883, 8
  %fn_ret_3885 = call i64 @__omni_cyw_priv_w8(i64 %t_3884, i64 88, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_3886 = load i64, i64* %var_io_3860
  %t_3887 = add i64 %t64_3886, 9
  %fn_ret_3888 = call i64 @__omni_cyw_priv_w8(i64 %t_3887, i64 88, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_3889 = load i64, i64* %var_io_3860
  %t_3890 = add i64 %t64_3889, 12
  %fn_ret_3891 = call i64 @__omni_cyw_priv_w32_le(i64 %t_3890, i64 4294967295, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_3892 = load i64, i64* %var_io_3860
  %t_3893 = add i64 %t64_3892, 16
  %fn_ret_3894 = call i64 @__omni_cyw_priv_w8(i64 %t_3893, i64 88, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_3895 = load i64, i64* %var_io_3860
  %t_3896 = add i64 %t64_3895, 17
  %fn_ret_3897 = call i64 @__omni_cyw_priv_w8(i64 %t_3896, i64 88, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_3898 = load i64, i64* %var_ctx_3857
  %t64_3899 = load i64, i64* %var_io_3860
  %fn_ret_3900 = call i64 @__omni_fn_cyw_do_ioctl(i64 %t64_3898, i64 1, i64 263, i64 %t64_3899, i64 28, i64 -581005069656919554)
  %ext_i64_3901 = call i64 @sleep_ms(i64 50)
  %t64_3902 = load i64, i64* %var_ctx_3857
  %fn_ret_3903 = call i64 @__omni_cyw_priv_ioctl_set_u32(i64 %t64_3902, i64 64, i64 0, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_3904 = load i64, i64* %var_ctx_3857
  %str_lit_3905 = getelementptr inbounds [11 x i8], [11 x i8]* @.str.cyw_14, i64 0, i64 0
  call void @__omni_c_return_ptr(i8* %str_lit_3905)
  %p2i64_3906 = ptrtoint i8* %str_lit_3905 to i64
  %fn_ret_3907 = call i64 @__omni_cyw_priv_iovar_set_u32(i64 %t64_3904, i64 %p2i64_3906, i64 0, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_3908 = load i64, i64* %var_ctx_3857
  %str_lit_3909 = getelementptr inbounds [6 x i8], [6 x i8]* @.str.cyw_15, i64 0, i64 0
  call void @__omni_c_return_ptr(i8* %str_lit_3909)
  %p2i64_3910 = ptrtoint i8* %str_lit_3909 to i64
  %fn_ret_3911 = call i64 @__omni_cyw_priv_iovar_set_u32(i64 %t64_3908, i64 %p2i64_3910, i64 1, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_3912 = load i64, i64* %var_ctx_3857
  %str_lit_3913 = getelementptr inbounds [15 x i8], [15 x i8]* @.str.cyw_16, i64 0, i64 0
  call void @__omni_c_return_ptr(i8* %str_lit_3913)
  %p2i64_3914 = ptrtoint i8* %str_lit_3913 to i64
  %fn_ret_3915 = call i64 @__omni_cyw_priv_iovar_set_u32(i64 %t64_3912, i64 %p2i64_3914, i64 8, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_3916 = load i64, i64* %var_ctx_3857
  %str_lit_3917 = getelementptr inbounds [11 x i8], [11 x i8]* @.str.cyw_17, i64 0, i64 0
  call void @__omni_c_return_ptr(i8* %str_lit_3917)
  %p2i64_3918 = ptrtoint i8* %str_lit_3917 to i64
  %fn_ret_3919 = call i64 @__omni_cyw_priv_iovar_set_u32(i64 %t64_3916, i64 %p2i64_3918, i64 4, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_3920 = load i64, i64* %var_ctx_3857
  %str_lit_3921 = getelementptr inbounds [16 x i8], [16 x i8]* @.str.cyw_18, i64 0, i64 0
  call void @__omni_c_return_ptr(i8* %str_lit_3921)
  %p2i64_3922 = ptrtoint i8* %str_lit_3921 to i64
  %fn_ret_3923 = call i64 @__omni_cyw_priv_iovar_set_u32(i64 %t64_3920, i64 %p2i64_3922, i64 0, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %ext_i64_3924 = call i64 @sleep_ms(i64 120)
  %t64_3925 = load i64, i64* %var_io_3860
  %fn_ret_3926 = call i64 @__omni_cyw_priv_bzero(i64 %t64_3925, i64 48, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  call void @__omni_c_return_ptr(i8* null)
  %t64_3927 = load i64, i64* %var_io_3860
  %str_lit_3928 = getelementptr inbounds [18 x i8], [18 x i8]* @.str.cyw_19, i64 0, i64 0
  call void @__omni_c_return_ptr(i8* %str_lit_3928)
  %p2i64_3929 = ptrtoint i8* %str_lit_3928 to i64
  %fn_ret_3930 = call i64 @__omni_cyw_priv_put_ascii(i64 %t64_3927, i64 %p2i64_3929, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  store i64 %fn_ret_3930, i64* %var_el_3931
  %t64_3932 = load i64, i64* %var_io_3860
  %t64_3933 = load i64, i64* %var_el_3931
  %t_3934 = add i64 %t64_3932, %t64_3933
  %fn_ret_3935 = call i64 @__omni_cyw_priv_w8(i64 %t_3934, i64 0, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_3936 = load i64, i64* %var_io_3860
  %t_3937 = add i64 %t64_3936, 18
  %fn_ret_3938 = call i64 @__omni_cyw_priv_w32_le(i64 %t_3937, i64 0, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  call void @__omni_c_return_ptr(i8* null)
  store i64 0, i64* %var_mi_3939
  br label %while_cond_180

while_cond_180:
  %t64_3940 = load i64, i64* %var_mi_3939
  %cond_3941 = icmp slt i64 %t64_3940, 19
  br i1 %cond_3941, label %while_body_180, label %while_exit_180

while_body_180:
  %t64_3942 = load i64, i64* %var_io_3860
  %t_3943 = add i64 %t64_3942, 22
  %t64_3944 = load i64, i64* %var_mi_3939
  %t_3945 = add i64 %t_3943, %t64_3944
  %fn_ret_3946 = call i64 @__omni_cyw_priv_w8(i64 %t_3945, i64 255, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  call void @__omni_c_return_ptr(i8* null)
  %t64_3947 = load i64, i64* %var_mi_3939
  %t_3948 = add i64 %t64_3947, 1
  store i64 %t_3948, i64* %var_mi_3939
  br label %while_cond_180

while_exit_180:
  %t64_3949 = load i64, i64* %var_io_3860
  %t_3950 = add i64 %t64_3949, 22
  %t_3951 = add i64 %t_3950, 2
  %fn_ret_3952 = call i64 @__omni_cyw_priv_w8(i64 %t_3951, i64 231, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_3953 = load i64, i64* %var_io_3860
  %t_3954 = add i64 %t64_3953, 22
  %t_3955 = add i64 %t_3954, 5
  %fn_ret_3956 = call i64 @__omni_cyw_priv_w8(i64 %t_3955, i64 238, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_3957 = load i64, i64* %var_io_3860
  %t_3958 = add i64 %t64_3957, 22
  %t_3959 = add i64 %t_3958, 6
  %fn_ret_3960 = call i64 @__omni_cyw_priv_w8(i64 %t_3959, i64 191, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_3961 = load i64, i64* %var_io_3860
  %t_3962 = add i64 %t64_3961, 22
  %t_3963 = add i64 %t_3962, 8
  %fn_ret_3964 = call i64 @__omni_cyw_priv_w8(i64 %t_3963, i64 127, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_3965 = load i64, i64* %var_ctx_3857
  %t64_3966 = load i64, i64* %var_io_3860
  %fn_ret_3967 = call i64 @__omni_fn_cyw_do_ioctl(i64 %t64_3965, i64 1, i64 263, i64 %t64_3966, i64 41, i64 -581005069656919554)
  %ext_i64_3968 = call i64 @sleep_ms(i64 40)
  %t64_3969 = load i64, i64* %var_ctx_3857
  %fn_ret_3970 = call i64 @__omni_fn_cyw_do_ioctl(i64 %t64_3969, i64 1, i64 2, i64 0, i64 0, i64 -581005069656919554)
  %ext_i64_3971 = call i64 @sleep_ms(i64 50)
  %t64_3972 = load i64, i64* %var_io_3860
  %fn_ret_3973 = call i64 @__omni_cyw_priv_bzero(i64 %t64_3972, i64 32, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  call void @__omni_c_return_ptr(i8* null)
  %t64_3974 = load i64, i64* %var_io_3860
  %str_lit_3975 = getelementptr inbounds [14 x i8], [14 x i8]* @.str.cyw_20, i64 0, i64 0
  call void @__omni_c_return_ptr(i8* %str_lit_3975)
  %p2i64_3976 = ptrtoint i8* %str_lit_3975 to i64
  %fn_ret_3977 = call i64 @__omni_cyw_priv_put_ascii(i64 %t64_3974, i64 %p2i64_3976, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  store i64 %fn_ret_3977, i64* %var_ml_3978
  %t64_3979 = load i64, i64* %var_io_3860
  %t64_3980 = load i64, i64* %var_ml_3978
  %t_3981 = add i64 %t64_3979, %t64_3980
  %fn_ret_3982 = call i64 @__omni_cyw_priv_w8(i64 %t_3981, i64 0, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_3983 = load i64, i64* %var_ctx_3857
  %t64_3984 = load i64, i64* %var_io_3860
  %fn_ret_3985 = call i64 @__omni_fn_cyw_do_ioctl(i64 %t64_3983, i64 0, i64 262, i64 %t64_3984, i64 20, i64 -581005069656919554)
  %cond_3986 = icmp eq i64 %fn_ret_3985, 0
  br i1 %cond_3986, label %if_then_181, label %if_merge_181

if_then_181:
  %t64_3987 = load i64, i64* %var_ctx_3857
  %t_3988 = add i64 %t64_3987, 32
  %t64_3989 = load i64, i64* %var_io_3860
  %fn_ret_3990 = call i64 @__omni_cyw_priv_bcopy(i64 %t_3988, i64 %t64_3989, i64 6, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  br label %if_merge_181

if_merge_181:
  %t64_3991 = load i64, i64* %var_ctx_3857
  %t_3992 = add i64 %t64_3991, 32
  %fn_ret_3993 = call i64 @__omni_cyw_priv_r8(i64 %t_3992, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_3994 = load i64, i64* %var_ctx_3857
  %t_3995 = add i64 %t64_3994, 33
  %fn_ret_3996 = call i64 @__omni_cyw_priv_r8(i64 %t_3995, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t_3997 = or i64 %fn_ret_3993, %fn_ret_3996
  %t64_3998 = load i64, i64* %var_ctx_3857
  %t_3999 = add i64 %t64_3998, 34
  %fn_ret_4000 = call i64 @__omni_cyw_priv_r8(i64 %t_3999, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t_4001 = or i64 %t_3997, %fn_ret_4000
  %t64_4002 = load i64, i64* %var_ctx_3857
  %t_4003 = add i64 %t64_4002, 35
  %fn_ret_4004 = call i64 @__omni_cyw_priv_r8(i64 %t_4003, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t_4005 = or i64 %t_4001, %fn_ret_4004
  %t64_4006 = load i64, i64* %var_ctx_3857
  %t_4007 = add i64 %t64_4006, 36
  %fn_ret_4008 = call i64 @__omni_cyw_priv_r8(i64 %t_4007, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t_4009 = or i64 %t_4005, %fn_ret_4008
  %t64_4010 = load i64, i64* %var_ctx_3857
  %t_4011 = add i64 %t64_4010, 37
  %fn_ret_4012 = call i64 @__omni_cyw_priv_r8(i64 %t_4011, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t_4013 = or i64 %t_4009, %fn_ret_4012
  %cond_4014 = icmp eq i64 %t_4013, 0
  br i1 %cond_4014, label %if_then_182, label %if_merge_182

if_then_182:
  %t64_4015 = load i64, i64* %var_ctx_3857
  %t_4016 = add i64 %t64_4015, 32
  %fn_ret_4017 = call i64 @__omni_cyw_priv_w8(i64 %t_4016, i64 2, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_4018 = load i64, i64* %var_ctx_3857
  %t_4019 = add i64 %t64_4018, 33
  %fn_ret_4020 = call i64 @__omni_cyw_priv_w8(i64 %t_4019, i64 79, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_4021 = load i64, i64* %var_ctx_3857
  %t_4022 = add i64 %t64_4021, 34
  %fn_ret_4023 = call i64 @__omni_cyw_priv_w8(i64 %t_4022, i64 77, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_4024 = load i64, i64* %var_ctx_3857
  %t_4025 = add i64 %t64_4024, 35
  %fn_ret_4026 = call i64 @__omni_cyw_priv_w8(i64 %t_4025, i64 78, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_4027 = load i64, i64* %var_ctx_3857
  %t_4028 = add i64 %t64_4027, 36
  %fn_ret_4029 = call i64 @__omni_cyw_priv_w8(i64 %t_4028, i64 73, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_4030 = load i64, i64* %var_ctx_3857
  %t_4031 = add i64 %t64_4030, 37
  %fn_ret_4032 = call i64 @__omni_cyw_priv_w8(i64 %t_4031, i64 1, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  br label %if_merge_182

if_merge_182:
  %t64_4033 = load i64, i64* %var_ctx_3857
  %t_4034 = add i64 %t64_4033, 0
  %ext_i64_4035 = call i64 @mmio_write32(i64 %t_4034, i64 1)
  br label %if_merge_178

if_merge_178:
  call void @__omni_c_return_ptr(i8* null)
  %t64_4036 = load i64, i64* %param_ssid_ptr_3853
  %fn_ret_4037 = call i64 @__omni_cyw_priv_cstr_len(i64 %t64_4036, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  store i64 %fn_ret_4037, i64* %var_ssid_len_4038
  %t64_4039 = load i64, i64* %var_ssid_len_4038
  %cond_4040 = icmp sgt i64 %t64_4039, 32
  br i1 %cond_4040, label %if_then_183, label %if_merge_183

if_then_183:
  call void @__omni_c_return_ptr(i8* null)
  store i64 32, i64* %var_ssid_len_4038
  br label %if_merge_183

if_merge_183:
  call void @__omni_c_return_ptr(i8* null)
  %t64_4041 = load i64, i64* %param_pass_ptr_3855
  %fn_ret_4042 = call i64 @__omni_cyw_priv_cstr_len(i64 %t64_4041, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  store i64 %fn_ret_4042, i64* %var_pass_len_4043
  %t64_4044 = load i64, i64* %var_pass_len_4043
  %cond_4045 = icmp sgt i64 %t64_4044, 64
  br i1 %cond_4045, label %if_then_184, label %if_merge_184

if_then_184:
  call void @__omni_c_return_ptr(i8* null)
  store i64 64, i64* %var_pass_len_4043
  br label %if_merge_184

if_merge_184:
  call void @__omni_c_return_ptr(i8* null)
  store i64 0, i64* %var_attempt_4046
  call void @__omni_c_return_ptr(i8* null)
  store i64 0, i64* %var_joined_4047
  call void @__omni_c_return_ptr(i8* null)
  store i64 1, i64* %var_led_4048
  br label %while_cond_185

while_cond_185:
  %t64_4050 = load i64, i64* %var_attempt_4046
  %cond_4051 = icmp slt i64 %t64_4050, 3
  br i1 %cond_4051, label %sc_and_rhs_4049, label %sc_and_short_4049

sc_and_short_4049:
  br label %sc_and_merge_4049

sc_and_rhs_4049:
  %t64_4052 = load i64, i64* %var_joined_4047
  %cond_4053 = icmp eq i64 %t64_4052, 0
  br label %sc_and_done_4049

sc_and_done_4049:
  br label %sc_and_merge_4049

sc_and_merge_4049:
  %and_cond_4054 = phi i1 [ false, %sc_and_short_4049 ], [ %cond_4053, %sc_and_done_4049 ]
  br i1 %and_cond_4054, label %while_body_185, label %while_exit_185

while_body_185:
  %t64_4055 = load i64, i64* %var_ctx_3857
  %t_4056 = add i64 %t64_4055, 4
  %ext_i64_4057 = call i64 @mmio_write32(i64 %t_4056, i64 0)
  %t64_4058 = load i64, i64* %var_ctx_3857
  %fn_ret_4059 = call i64 @__omni_cyw_priv_ioctl_set_u32(i64 %t64_4058, i64 110, i64 1, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_4060 = load i64, i64* %var_ctx_3857
  %fn_ret_4061 = call i64 @__omni_cyw_priv_ioctl_set_u32(i64 %t64_4060, i64 142, i64 0, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_4062 = load i64, i64* %var_ctx_3857
  %str_lit_4063 = getelementptr inbounds [14 x i8], [14 x i8]* @.str.cyw_21, i64 0, i64 0
  call void @__omni_c_return_ptr(i8* %str_lit_4063)
  %p2i64_4064 = ptrtoint i8* %str_lit_4063 to i64
  %fn_ret_4065 = call i64 @__omni_cyw_priv_iovar_set_u32(i64 %t64_4062, i64 %p2i64_4064, i64 200, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_4066 = load i64, i64* %var_ctx_3857
  %str_lit_4067 = getelementptr inbounds [11 x i8], [11 x i8]* @.str.cyw_22, i64 0, i64 0
  call void @__omni_c_return_ptr(i8* %str_lit_4067)
  %p2i64_4068 = ptrtoint i8* %str_lit_4067 to i64
  %fn_ret_4069 = call i64 @__omni_cyw_priv_iovar_set_u32(i64 %t64_4066, i64 %p2i64_4068, i64 1, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_4070 = load i64, i64* %var_ctx_3857
  %str_lit_4071 = getelementptr inbounds [12 x i8], [12 x i8]* @.str.cyw_23, i64 0, i64 0
  call void @__omni_c_return_ptr(i8* %str_lit_4071)
  %p2i64_4072 = ptrtoint i8* %str_lit_4071 to i64
  %fn_ret_4073 = call i64 @__omni_cyw_priv_iovar_set_u32(i64 %t64_4070, i64 %p2i64_4072, i64 1, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_4074 = load i64, i64* %var_ctx_3857
  %str_lit_4075 = getelementptr inbounds [13 x i8], [13 x i8]* @.str.cyw_24, i64 0, i64 0
  call void @__omni_c_return_ptr(i8* %str_lit_4075)
  %p2i64_4076 = ptrtoint i8* %str_lit_4075 to i64
  %fn_ret_4077 = call i64 @__omni_cyw_priv_iovar_set_u32(i64 %t64_4074, i64 %p2i64_4076, i64 10, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_4078 = load i64, i64* %var_pass_len_4043
  %cond_4079 = icmp sgt i64 %t64_4078, 0
  br i1 %cond_4079, label %if_then_186, label %if_else_186

if_then_186:
  call void @__omni_c_return_ptr(i8* null)
  store i64 6, i64* %var_wsec_4080
  %t64_4081 = load i64, i64* %var_attempt_4046
  %cond_4082 = icmp eq i64 %t64_4081, 1
  br i1 %cond_4082, label %if_then_187, label %if_merge_187

if_then_187:
  call void @__omni_c_return_ptr(i8* null)
  store i64 4, i64* %var_wsec_4080
  br label %if_merge_187

if_merge_187:
  %t64_4083 = load i64, i64* %var_ctx_3857
  %t64_4084 = load i64, i64* %var_wsec_4080
  %fn_ret_4085 = call i64 @__omni_cyw_priv_ioctl_set_u32(i64 %t64_4083, i64 134, i64 %t64_4084, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_4086 = load i64, i64* %var_ctx_3857
  %str_lit_4087 = getelementptr inbounds [15 x i8], [15 x i8]* @.str.cyw_25, i64 0, i64 0
  call void @__omni_c_return_ptr(i8* %str_lit_4087)
  %p2i64_4088 = ptrtoint i8* %str_lit_4087 to i64
  %fn_ret_4089 = call i64 @__omni_cyw_priv_iovar_set_u32x2(i64 %t64_4086, i64 %p2i64_4088, i64 0, i64 1, i64 -581005069656919554, i64 -581005069656919554)
  %t64_4090 = load i64, i64* %var_ctx_3857
  %str_lit_4091 = getelementptr inbounds [23 x i8], [23 x i8]* @.str.cyw_26, i64 0, i64 0
  call void @__omni_c_return_ptr(i8* %str_lit_4091)
  %p2i64_4092 = ptrtoint i8* %str_lit_4091 to i64
  %fn_ret_4093 = call i64 @__omni_cyw_priv_iovar_set_u32x2(i64 %t64_4090, i64 %p2i64_4092, i64 0, i64 4294967295, i64 -581005069656919554, i64 -581005069656919554)
  %t64_4094 = load i64, i64* %var_ctx_3857
  %str_lit_4095 = getelementptr inbounds [19 x i8], [19 x i8]* @.str.cyw_27, i64 0, i64 0
  call void @__omni_c_return_ptr(i8* %str_lit_4095)
  %p2i64_4096 = ptrtoint i8* %str_lit_4095 to i64
  %fn_ret_4097 = call i64 @__omni_cyw_priv_iovar_set_u32x2(i64 %t64_4094, i64 %p2i64_4096, i64 0, i64 2500, i64 -581005069656919554, i64 -581005069656919554)
  %ext_i64_4098 = call i64 @sleep_ms(i64 5)
  %t64_4099 = load i64, i64* %var_io_3860
  %fn_ret_4100 = call i64 @__omni_cyw_priv_bzero(i64 %t64_4099, i64 72, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_4101 = load i64, i64* %var_io_3860
  %t_4102 = add i64 %t64_4101, 0
  %t64_4103 = load i64, i64* %var_pass_len_4043
  %fn_ret_4104 = call i64 @__omni_cyw_priv_w16_le(i64 %t_4102, i64 %t64_4103, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_4105 = load i64, i64* %var_io_3860
  %t_4106 = add i64 %t64_4105, 2
  %fn_ret_4107 = call i64 @__omni_cyw_priv_w16_le(i64 %t_4106, i64 1, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_4108 = load i64, i64* %var_io_3860
  %t_4109 = add i64 %t64_4108, 4
  %t64_4110 = load i64, i64* %param_pass_ptr_3855
  %t64_4111 = load i64, i64* %var_pass_len_4043
  %fn_ret_4112 = call i64 @__omni_cyw_priv_bcopy(i64 %t_4109, i64 %t64_4110, i64 %t64_4111, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_4113 = load i64, i64* %var_ctx_3857
  %t64_4114 = load i64, i64* %var_io_3860
  %fn_ret_4115 = call i64 @__omni_fn_cyw_do_ioctl(i64 %t64_4113, i64 1, i64 268, i64 %t64_4114, i64 68, i64 -581005069656919554)
  %t64_4116 = load i64, i64* %var_ctx_3857
  %fn_ret_4117 = call i64 @__omni_cyw_priv_ioctl_set_u32(i64 %t64_4116, i64 20, i64 1, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_4118 = load i64, i64* %var_ctx_3857
  %fn_ret_4119 = call i64 @__omni_cyw_priv_ioctl_set_u32(i64 %t64_4118, i64 22, i64 0, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  call void @__omni_c_return_ptr(i8* null)
  store i64 1, i64* %var_mfp_val_4120
  %t64_4121 = load i64, i64* %var_attempt_4046
  %cond_4122 = icmp eq i64 %t64_4121, 2
  br i1 %cond_4122, label %if_then_188, label %if_merge_188

if_then_188:
  call void @__omni_c_return_ptr(i8* null)
  store i64 0, i64* %var_mfp_val_4120
  br label %if_merge_188

if_merge_188:
  %t64_4123 = load i64, i64* %var_ctx_3857
  %str_lit_4124 = getelementptr inbounds [4 x i8], [4 x i8]* @.str.cyw_28, i64 0, i64 0
  call void @__omni_c_return_ptr(i8* %str_lit_4124)
  %p2i64_4125 = ptrtoint i8* %str_lit_4124 to i64
  %t64_4126 = load i64, i64* %var_mfp_val_4120
  %fn_ret_4127 = call i64 @__omni_cyw_priv_iovar_set_u32(i64 %t64_4123, i64 %p2i64_4125, i64 %t64_4126, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_4128 = load i64, i64* %var_ctx_3857
  %fn_ret_4129 = call i64 @__omni_cyw_priv_ioctl_set_u32(i64 %t64_4128, i64 165, i64 128, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  br label %if_merge_186

if_else_186:
  %t64_4130 = load i64, i64* %var_ctx_3857
  %fn_ret_4131 = call i64 @__omni_cyw_priv_ioctl_set_u32(i64 %t64_4130, i64 134, i64 0, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_4132 = load i64, i64* %var_ctx_3857
  %fn_ret_4133 = call i64 @__omni_cyw_priv_ioctl_set_u32(i64 %t64_4132, i64 20, i64 1, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_4134 = load i64, i64* %var_ctx_3857
  %fn_ret_4135 = call i64 @__omni_cyw_priv_ioctl_set_u32(i64 %t64_4134, i64 22, i64 0, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_4136 = load i64, i64* %var_ctx_3857
  %fn_ret_4137 = call i64 @__omni_cyw_priv_ioctl_set_u32(i64 %t64_4136, i64 165, i64 0, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  br label %if_merge_186

if_merge_186:
  %t64_4138 = load i64, i64* %var_io_3860
  %fn_ret_4139 = call i64 @__omni_cyw_priv_bzero(i64 %t64_4138, i64 40, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_4140 = load i64, i64* %var_io_3860
  %t64_4141 = load i64, i64* %var_ssid_len_4038
  %fn_ret_4142 = call i64 @__omni_cyw_priv_w32_le(i64 %t64_4140, i64 %t64_4141, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_4143 = load i64, i64* %var_io_3860
  %t_4144 = add i64 %t64_4143, 4
  %t64_4145 = load i64, i64* %param_ssid_ptr_3853
  %t64_4146 = load i64, i64* %var_ssid_len_4038
  %fn_ret_4147 = call i64 @__omni_cyw_priv_bcopy(i64 %t_4144, i64 %t64_4145, i64 %t64_4146, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_4148 = load i64, i64* %var_ctx_3857
  %t64_4149 = load i64, i64* %var_io_3860
  %fn_ret_4150 = call i64 @__omni_fn_cyw_do_ioctl(i64 %t64_4148, i64 1, i64 26, i64 %t64_4149, i64 36, i64 -581005069656919554)
  call void @__omni_c_return_ptr(i8* null)
  store i64 0, i64* %var_wait_ms_4151
  br label %while_cond_189

while_cond_189:
  %t64_4152 = load i64, i64* %var_wait_ms_4151
  %cond_4153 = icmp slt i64 %t64_4152, 9000
  br i1 %cond_4153, label %while_body_189, label %while_exit_189

while_body_189:
  call void @__omni_c_return_ptr(i8* null)
  store i64 0, i64* %var_drain_k_4154
  br label %while_cond_190

while_cond_190:
  %t64_4155 = load i64, i64* %var_drain_k_4154
  %cond_4156 = icmp slt i64 %t64_4155, 8
  br i1 %cond_4156, label %while_body_190, label %while_exit_190

while_body_190:
  %t64_4157 = load i64, i64* %var_ctx_3857
  %fn_ret_4158 = call i64 @__omni_fn_cyw_poll_one_pkt(i64 %t64_4157, i64 0, i64 0, i64 0, i64 -581005069656919554, i64 -581005069656919554)
  %cond_4159 = icmp eq i64 %fn_ret_4158, 0
  br i1 %cond_4159, label %if_then_191, label %if_merge_191

if_then_191:
  br label %while_exit_190
if_merge_191:
  call void @__omni_c_return_ptr(i8* null)
  %t64_4160 = load i64, i64* %var_drain_k_4154
  %t_4161 = add i64 %t64_4160, 1
  store i64 %t_4161, i64* %var_drain_k_4154
  br label %while_cond_190

while_exit_190:
  call void @__omni_c_return_ptr(i8* null)
  %t64_4162 = load i64, i64* %var_ctx_3857
  %t_4163 = add i64 %t64_4162, 4
  %ext_i64_4164 = call i64 @mmio_read32(i64 %t_4163)
  store i64 %ext_i64_4164, i64* %var_jf_4165
  %t64_4166 = load i64, i64* %var_pass_len_4043
  %cond_4167 = icmp sgt i64 %t64_4166, 0
  br i1 %cond_4167, label %if_then_192, label %if_else_192

if_then_192:
  %t64_4168 = load i64, i64* %var_jf_4165
  %t_4169 = and i64 %t64_4168, 3
  %cond_4170 = icmp eq i64 %t_4169, 3
  br i1 %cond_4170, label %if_then_193, label %if_merge_193

if_then_193:
  call void @__omni_c_return_ptr(i8* null)
  store i64 1, i64* %var_joined_4047
  br label %while_exit_189
if_merge_193:
  br label %if_merge_192

if_else_192:
  %t64_4171 = load i64, i64* %var_jf_4165
  %t_4172 = and i64 %t64_4171, 1
  %cond_4173 = icmp eq i64 %t_4172, 1
  br i1 %cond_4173, label %if_then_194, label %if_merge_194

if_then_194:
  call void @__omni_c_return_ptr(i8* null)
  store i64 1, i64* %var_joined_4047
  br label %while_exit_189
if_merge_194:
  br label %if_merge_192

if_merge_192:
  %t64_4175 = load i64, i64* %var_jf_4165
  %t_4176 = and i64 %t64_4175, 4
  %cond_4177 = icmp ne i64 %t_4176, 0
  br i1 %cond_4177, label %sc_and_rhs_4174, label %sc_and_short_4174

sc_and_short_4174:
  br label %sc_and_merge_4174

sc_and_rhs_4174:
  %t64_4178 = load i64, i64* %var_wait_ms_4151
  %cond_4179 = icmp sgt i64 %t64_4178, 2000
  br label %sc_and_done_4174

sc_and_done_4174:
  br label %sc_and_merge_4174

sc_and_merge_4174:
  %and_cond_4180 = phi i1 [ false, %sc_and_short_4174 ], [ %cond_4179, %sc_and_done_4174 ]
  br i1 %and_cond_4180, label %if_then_195, label %if_merge_195

if_then_195:
  br label %while_exit_189
if_merge_195:
  %t64_4181 = load i64, i64* %var_wait_ms_4151
  %t_4182 = and i64 %t64_4181, 127
  %cond_4183 = icmp eq i64 %t_4182, 0
  br i1 %cond_4183, label %if_then_196, label %if_merge_196

if_then_196:
  %t64_4184 = load i64, i64* %var_led_4048
  %fn_ret_4185 = call i64 @__omni_fn_cyw_net_gpio_put(i64 0, i64 %t64_4184, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  call void @__omni_c_return_ptr(i8* null)
  %t64_4186 = load i64, i64* %var_led_4048
  %t_4187 = sub i64 1, %t64_4186
  store i64 %t_4187, i64* %var_led_4048
  br label %if_merge_196

if_merge_196:
  %ext_i64_4188 = call i64 @sleep_ms(i64 4)
  call void @__omni_c_return_ptr(i8* null)
  %t64_4189 = load i64, i64* %var_wait_ms_4151
  %t_4190 = add i64 %t64_4189, 4
  store i64 %t_4190, i64* %var_wait_ms_4151
  br label %while_cond_189

while_exit_189:
  %t64_4191 = load i64, i64* %var_joined_4047
  %cond_4192 = icmp eq i64 %t64_4191, 0
  br i1 %cond_4192, label %if_then_197, label %if_merge_197

if_then_197:
  %t64_4193 = load i64, i64* %var_ctx_3857
  %fn_ret_4194 = call i64 @__omni_fn_cyw_do_ioctl(i64 %t64_4193, i64 1, i64 3, i64 0, i64 0, i64 -581005069656919554)
  %ext_i64_4195 = call i64 @sleep_ms(i64 150)
  %t64_4196 = load i64, i64* %var_ctx_3857
  %fn_ret_4197 = call i64 @__omni_fn_cyw_do_ioctl(i64 %t64_4196, i64 1, i64 2, i64 0, i64 0, i64 -581005069656919554)
  %ext_i64_4198 = call i64 @sleep_ms(i64 100)
  br label %if_merge_197

if_merge_197:
  call void @__omni_c_return_ptr(i8* null)
  %t64_4199 = load i64, i64* %var_attempt_4046
  %t_4200 = add i64 %t64_4199, 1
  store i64 %t_4200, i64* %var_attempt_4046
  br label %while_cond_185

while_exit_185:
  %t64_4201 = load i64, i64* %var_joined_4047
  %cond_4202 = icmp eq i64 %t64_4201, 0
  br i1 %cond_4202, label %if_then_198, label %if_merge_198

if_then_198:
  %fn_ret_4203 = call i64 @__omni_fn_cyw_net_gpio_put(i64 0, i64 0, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  ret i64 -10
if_merge_198:
  %t64_4204 = load i64, i64* %var_ctx_3857
  %t_4205 = add i64 %t64_4204, 8
  %ext_i64_4206 = call i64 @mmio_write32(i64 %t_4205, i64 0)
  %t64_4207 = load i64, i64* %var_ctx_3857
  %t_4208 = add i64 %t64_4207, 60
  %ext_i64_4209 = call i64 @mmio_write32(i64 %t_4208, i64 0)
  %t64_4210 = load i64, i64* %var_ctx_3857
  %t_4211 = add i64 %t64_4210, 24
  %t64_4212 = load i64, i64* %var_ctx_3857
  %t_4213 = add i64 %t64_4212, 24
  %ext_i64_4214 = call i64 @mmio_read32(i64 %t_4213)
  %t_4215 = add i64 %ext_i64_4214, 1
  %t_4216 = and i64 %t_4215, 4294967295
  %ext_i64_4217 = call i64 @mmio_write32(i64 %t_4211, i64 %t_4216)
  call void @__omni_c_return_ptr(i8* null)
  store i64 0, i64* %var_dhcp_ms_4218
  call void @__omni_c_return_ptr(i8* null)
  store i64 0, i64* %var_next_tx_ms_4219
  br label %while_cond_199

while_cond_199:
  %t64_4220 = load i64, i64* %var_dhcp_ms_4218
  %cond_4221 = icmp slt i64 %t64_4220, 12000
  br i1 %cond_4221, label %while_body_199, label %while_exit_199

while_body_199:
  call void @__omni_c_return_ptr(i8* null)
  store i64 0, i64* %var_dk_4222
  br label %while_cond_200

while_cond_200:
  %t64_4223 = load i64, i64* %var_dk_4222
  %cond_4224 = icmp slt i64 %t64_4223, 8
  br i1 %cond_4224, label %while_body_200, label %while_exit_200

while_body_200:
  %t64_4225 = load i64, i64* %var_ctx_3857
  %fn_ret_4226 = call i64 @__omni_fn_cyw_poll_one_pkt(i64 %t64_4225, i64 0, i64 0, i64 0, i64 -581005069656919554, i64 -581005069656919554)
  %cond_4227 = icmp eq i64 %fn_ret_4226, 0
  br i1 %cond_4227, label %if_then_201, label %if_merge_201

if_then_201:
  br label %while_exit_200
if_merge_201:
  call void @__omni_c_return_ptr(i8* null)
  %t64_4228 = load i64, i64* %var_dk_4222
  %t_4229 = add i64 %t64_4228, 1
  store i64 %t_4229, i64* %var_dk_4222
  br label %while_cond_200

while_exit_200:
  call void @__omni_c_return_ptr(i8* null)
  %t64_4230 = load i64, i64* %var_ctx_3857
  %t_4231 = add i64 %t64_4230, 8
  %ext_i64_4232 = call i64 @mmio_read32(i64 %t_4231)
  store i64 %ext_i64_4232, i64* %var_dst_4233
  %t64_4234 = load i64, i64* %var_dst_4233
  %cond_4235 = icmp eq i64 %t64_4234, 4
  br i1 %cond_4235, label %if_then_202, label %if_merge_202

if_then_202:
  br label %while_exit_199
if_merge_202:
  %t64_4236 = load i64, i64* %var_dst_4233
  %cond_4237 = icmp eq i64 %t64_4236, 2
  br i1 %cond_4237, label %if_then_203, label %if_else_203

if_then_203:
  %t64_4238 = load i64, i64* %var_ctx_3857
  %fn_ret_4239 = call i64 @__omni_cyw_priv_dhcp_send(i64 %t64_4238, i64 3, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_4240 = load i64, i64* %var_ctx_3857
  %t_4241 = add i64 %t64_4240, 8
  %ext_i64_4242 = call i64 @mmio_write32(i64 %t_4241, i64 3)
  call void @__omni_c_return_ptr(i8* null)
  %t64_4243 = load i64, i64* %var_dhcp_ms_4218
  %t_4244 = add i64 %t64_4243, 1500
  store i64 %t_4244, i64* %var_next_tx_ms_4219
  br label %if_merge_203

if_else_203:
  %t64_4245 = load i64, i64* %var_dhcp_ms_4218
  %t64_4246 = load i64, i64* %var_next_tx_ms_4219
  %cond_4247 = icmp sge i64 %t64_4245, %t64_4246
  br i1 %cond_4247, label %if_then_204, label %if_merge_204

if_then_204:
  %t64_4248 = load i64, i64* %var_dst_4233
  %cond_4249 = icmp eq i64 %t64_4248, 3
  br i1 %cond_4249, label %if_then_205, label %if_else_205

if_then_205:
  %t64_4250 = load i64, i64* %var_ctx_3857
  %fn_ret_4251 = call i64 @__omni_cyw_priv_dhcp_send(i64 %t64_4250, i64 3, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  br label %if_merge_205

if_else_205:
  %t64_4252 = load i64, i64* %var_ctx_3857
  %fn_ret_4253 = call i64 @__omni_cyw_priv_dhcp_send(i64 %t64_4252, i64 1, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_4254 = load i64, i64* %var_ctx_3857
  %t_4255 = add i64 %t64_4254, 8
  %ext_i64_4256 = call i64 @mmio_write32(i64 %t_4255, i64 1)
  br label %if_merge_205

if_merge_205:
  call void @__omni_c_return_ptr(i8* null)
  %t64_4257 = load i64, i64* %var_dhcp_ms_4218
  %t_4258 = add i64 %t64_4257, 1500
  store i64 %t_4258, i64* %var_next_tx_ms_4219
  br label %if_merge_204

if_merge_204:
  br label %if_merge_203

if_merge_203:
  %t64_4259 = load i64, i64* %var_dhcp_ms_4218
  %t_4260 = and i64 %t64_4259, 63
  %cond_4261 = icmp eq i64 %t_4260, 0
  br i1 %cond_4261, label %if_then_206, label %if_merge_206

if_then_206:
  %t64_4262 = load i64, i64* %var_led_4048
  %fn_ret_4263 = call i64 @__omni_fn_cyw_net_gpio_put(i64 0, i64 %t64_4262, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  call void @__omni_c_return_ptr(i8* null)
  %t64_4264 = load i64, i64* %var_led_4048
  %t_4265 = sub i64 1, %t64_4264
  store i64 %t_4265, i64* %var_led_4048
  br label %if_merge_206

if_merge_206:
  %ext_i64_4266 = call i64 @sleep_ms(i64 4)
  call void @__omni_c_return_ptr(i8* null)
  %t64_4267 = load i64, i64* %var_dhcp_ms_4218
  %t_4268 = add i64 %t64_4267, 4
  store i64 %t_4268, i64* %var_dhcp_ms_4218
  br label %while_cond_199

while_exit_199:
  %t64_4269 = load i64, i64* %var_ctx_3857
  %t_4270 = add i64 %t64_4269, 8
  %ext_i64_4271 = call i64 @mmio_read32(i64 %t_4270)
  %cond_4272 = icmp ne i64 %ext_i64_4271, 4
  br i1 %cond_4272, label %if_then_207, label %if_merge_207

if_then_207:
  %fn_ret_4273 = call i64 @__omni_fn_cyw_net_gpio_put(i64 0, i64 0, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  ret i64 -11
if_merge_207:
  %fn_ret_4274 = call i64 @__omni_fn_cyw_net_gpio_put(i64 0, i64 1, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  call void @__omni_c_return_ptr(i8* null)
  %t64_4275 = load i64, i64* %var_ctx_3857
  %t_4276 = add i64 %t64_4275, 8000
  store i64 %t_4276, i64* %var_bmac_4277
  call void @__omni_c_return_ptr(i8* null)
  store i64 0, i64* %var_bi_4278
  br label %while_cond_208

while_cond_208:
  %t64_4279 = load i64, i64* %var_bi_4278
  %cond_4280 = icmp slt i64 %t64_4279, 6
  br i1 %cond_4280, label %while_body_208, label %while_exit_208

while_body_208:
  %t64_4281 = load i64, i64* %var_bmac_4277
  %t64_4282 = load i64, i64* %var_bi_4278
  %t_4283 = add i64 %t64_4281, %t64_4282
  %fn_ret_4284 = call i64 @__omni_cyw_priv_w8(i64 %t_4283, i64 255, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  call void @__omni_c_return_ptr(i8* null)
  %t64_4285 = load i64, i64* %var_bi_4278
  %t_4286 = add i64 %t64_4285, 1
  store i64 %t_4286, i64* %var_bi_4278
  br label %while_cond_208

while_exit_208:
  %t64_4287 = load i64, i64* %var_ctx_3857
  %t64_4288 = load i64, i64* %var_bmac_4277
  %t64_4289 = load i64, i64* %var_ctx_3857
  %t_4290 = add i64 %t64_4289, 40
  %fn_ret_4291 = call i64 @__omni_cyw_priv_send_arp_pkt(i64 %t64_4287, i64 %t64_4288, i64 %t_4290, i64 1, i64 -581005069656919554, i64 -581005069656919554)
  %t64_4292 = load i64, i64* %var_ctx_3857
  %fn_ret_4293 = call i64 @__omni_cyw_priv_broadcast_discovery(i64 %t64_4292, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %ext_i64_4294 = call i64 @sleep_ms(i64 30)
  %t64_4295 = load i64, i64* %var_ctx_3857
  %fn_ret_4296 = call i64 @__omni_cyw_priv_broadcast_discovery(i64 %t64_4295, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %t64_4297 = load i64, i64* %var_ctx_3857
  %t_4298 = add i64 %t64_4297, 28
  %fn_ret_4299 = call i64 @__omni_cyw_priv_now_ms(i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  %ext_i64_4300 = call i64 @mmio_write32(i64 %t_4298, i64 %fn_ret_4299)
  %t64_4301 = load i64, i64* %var_ctx_3857
  %t_4302 = add i64 %t64_4301, 40
  %fn_ret_4303 = call i64 @__omni_cyw_priv_r32_be(i64 %t_4302, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  ret i64 %fn_ret_4303
}


define void @__omni_trampoline_cyw_wifi_connect(i8* %raw_arg) {
entry:
  %raw_int = ptrtoint i8* %raw_arg to i64
  %res = call i64 @__omni_fn_cyw_wifi_connect(i64 %raw_int, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  ret void
}

define void @__omni_trampoline_pack_cyw_wifi_connect(i8* %raw_pack) {
entry:
  %p64 = bitcast i8* %raw_pack to i64*
  %p0 = getelementptr inbounds i64, i64* %p64, i64 0
  %a0 = load i64, i64* %p0
  %p1 = getelementptr inbounds i64, i64* %p64, i64 1
  %a1 = load i64, i64* %p1
  %p2 = getelementptr inbounds i64, i64* %p64, i64 2
  %a2 = load i64, i64* %p2
  %p3 = getelementptr inbounds i64, i64* %p64, i64 3
  %a3 = load i64, i64* %p3
  %p4 = getelementptr inbounds i64, i64* %p64, i64 4
  %a4 = load i64, i64* %p4
  %p5 = getelementptr inbounds i64, i64* %p64, i64 5
  %a5 = load i64, i64* %p5
  call void @free(i8* %raw_pack)
  %res = call i64 @__omni_fn_cyw_wifi_connect(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5)
  ret void
}

define i64 @__omni_fn_cyw_wifi_ip_ptr(i64 %arg0, i64 %arg1, i64 %arg2, i64 %arg3, i64 %arg4, i64 %arg5) {
entry:
  %var_ctx_4305 = alloca i64
  call void @__omni_c_return_ptr(i8* null)
  %fn_ret_4304 = call i64 @__omni_cyw_priv_get_net_ctx(i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  store i64 %fn_ret_4304, i64* %var_ctx_4305
  %t64_4306 = load i64, i64* %var_ctx_4305
  %t_4307 = add i64 %t64_4306, 128
  ret i64 %t_4307
}


define void @__omni_trampoline_cyw_wifi_ip_ptr(i8* %raw_arg) {
entry:
  %raw_int = ptrtoint i8* %raw_arg to i64
  %res = call i64 @__omni_fn_cyw_wifi_ip_ptr(i64 %raw_int, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  ret void
}

define void @__omni_trampoline_pack_cyw_wifi_ip_ptr(i8* %raw_pack) {
entry:
  %p64 = bitcast i8* %raw_pack to i64*
  %p0 = getelementptr inbounds i64, i64* %p64, i64 0
  %a0 = load i64, i64* %p0
  %p1 = getelementptr inbounds i64, i64* %p64, i64 1
  %a1 = load i64, i64* %p1
  %p2 = getelementptr inbounds i64, i64* %p64, i64 2
  %a2 = load i64, i64* %p2
  %p3 = getelementptr inbounds i64, i64* %p64, i64 3
  %a3 = load i64, i64* %p3
  %p4 = getelementptr inbounds i64, i64* %p64, i64 4
  %a4 = load i64, i64* %p4
  %p5 = getelementptr inbounds i64, i64* %p64, i64 5
  %a5 = load i64, i64* %p5
  call void @free(i8* %raw_pack)
  %res = call i64 @__omni_fn_cyw_wifi_ip_ptr(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5)
  ret void
}

define i64 @__omni_fn_cyw_ws_listen(i64 %arg0, i64 %arg1, i64 %arg2, i64 %arg3, i64 %arg4, i64 %arg5) {
entry:
  %param_port_4309 = alloca i64
  %var_ctx_4311 = alloca i64
  %param_unset_port_4308 = icmp eq i64 %arg0, -581005069656919554
  %param_dsel_port_4308 = select i1 %param_unset_port_4308, i64 0, i64 %arg0
  store i64 %param_dsel_port_4308, i64* %param_port_4309
  call void @__omni_c_return_ptr(i8* null)
  %fn_ret_4310 = call i64 @__omni_cyw_priv_get_net_ctx(i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  store i64 %fn_ret_4310, i64* %var_ctx_4311
  %t64_4312 = load i64, i64* %var_ctx_4311
  %t_4313 = add i64 %t64_4312, 124
  %ext_i64_4314 = call i64 @mmio_write32(i64 %t_4313, i64 0)
  %t64_4315 = load i64, i64* %var_ctx_4311
  %t_4316 = add i64 %t64_4315, 16
  %t64_4317 = load i64, i64* %param_port_4309
  %t_4318 = and i64 %t64_4317, 65535
  %ext_i64_4319 = call i64 @mmio_write32(i64 %t_4316, i64 %t_4318)
  %t64_4320 = load i64, i64* %var_ctx_4311
  %t_4321 = add i64 %t64_4320, 12
  %ext_i64_4322 = call i64 @mmio_write32(i64 %t_4321, i64 1)
  %t64_4323 = load i64, i64* %var_ctx_4311
  %t_4324 = add i64 %t64_4323, 100
  %ext_i64_4325 = call i64 @mmio_write32(i64 %t_4324, i64 0)
  %t64_4326 = load i64, i64* %var_ctx_4311
  %t_4327 = add i64 %t64_4326, 116
  %ext_i64_4328 = call i64 @mmio_write32(i64 %t_4327, i64 0)
  %t64_4329 = load i64, i64* %var_ctx_4311
  %t_4330 = add i64 %t64_4329, 120
  %ext_i64_4331 = call i64 @mmio_write32(i64 %t_4330, i64 0)
  %t64_4332 = load i64, i64* %var_ctx_4311
  %fn_ret_4333 = call i64 @__omni_cyw_priv_broadcast_discovery(i64 %t64_4332, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  ret i64 0
}


define void @__omni_trampoline_cyw_ws_listen(i8* %raw_arg) {
entry:
  %raw_int = ptrtoint i8* %raw_arg to i64
  %res = call i64 @__omni_fn_cyw_ws_listen(i64 %raw_int, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  ret void
}

define void @__omni_trampoline_pack_cyw_ws_listen(i8* %raw_pack) {
entry:
  %p64 = bitcast i8* %raw_pack to i64*
  %p0 = getelementptr inbounds i64, i64* %p64, i64 0
  %a0 = load i64, i64* %p0
  %p1 = getelementptr inbounds i64, i64* %p64, i64 1
  %a1 = load i64, i64* %p1
  %p2 = getelementptr inbounds i64, i64* %p64, i64 2
  %a2 = load i64, i64* %p2
  %p3 = getelementptr inbounds i64, i64* %p64, i64 3
  %a3 = load i64, i64* %p3
  %p4 = getelementptr inbounds i64, i64* %p64, i64 4
  %a4 = load i64, i64* %p4
  %p5 = getelementptr inbounds i64, i64* %p64, i64 5
  %a5 = load i64, i64* %p5
  call void @free(i8* %raw_pack)
  %res = call i64 @__omni_fn_cyw_ws_listen(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5)
  ret void
}

define i64 @__omni_fn_cyw_tcp_listen(i64 %arg0, i64 %arg1, i64 %arg2, i64 %arg3, i64 %arg4, i64 %arg5) {
entry:
  %param_port_4335 = alloca i64
  %var_ctx_4337 = alloca i64
  %param_unset_port_4334 = icmp eq i64 %arg0, -581005069656919554
  %param_dsel_port_4334 = select i1 %param_unset_port_4334, i64 0, i64 %arg0
  store i64 %param_dsel_port_4334, i64* %param_port_4335
  call void @__omni_c_return_ptr(i8* null)
  %fn_ret_4336 = call i64 @__omni_cyw_priv_get_net_ctx(i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  store i64 %fn_ret_4336, i64* %var_ctx_4337
  %t64_4338 = load i64, i64* %var_ctx_4337
  %t_4339 = add i64 %t64_4338, 124
  %ext_i64_4340 = call i64 @mmio_write32(i64 %t_4339, i64 1)
  %t64_4341 = load i64, i64* %var_ctx_4337
  %t_4342 = add i64 %t64_4341, 16
  %t64_4343 = load i64, i64* %param_port_4335
  %t_4344 = and i64 %t64_4343, 65535
  %ext_i64_4345 = call i64 @mmio_write32(i64 %t_4342, i64 %t_4344)
  %t64_4346 = load i64, i64* %var_ctx_4337
  %t_4347 = add i64 %t64_4346, 12
  %ext_i64_4348 = call i64 @mmio_write32(i64 %t_4347, i64 1)
  %t64_4349 = load i64, i64* %var_ctx_4337
  %t_4350 = add i64 %t64_4349, 100
  %ext_i64_4351 = call i64 @mmio_write32(i64 %t_4350, i64 0)
  %t64_4352 = load i64, i64* %var_ctx_4337
  %t_4353 = add i64 %t64_4352, 116
  %ext_i64_4354 = call i64 @mmio_write32(i64 %t_4353, i64 0)
  %t64_4355 = load i64, i64* %var_ctx_4337
  %t_4356 = add i64 %t64_4355, 120
  %ext_i64_4357 = call i64 @mmio_write32(i64 %t_4356, i64 0)
  %t64_4358 = load i64, i64* %var_ctx_4337
  %fn_ret_4359 = call i64 @__omni_cyw_priv_broadcast_discovery(i64 %t64_4358, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  ret i64 0
}


define void @__omni_trampoline_cyw_tcp_listen(i8* %raw_arg) {
entry:
  %raw_int = ptrtoint i8* %raw_arg to i64
  %res = call i64 @__omni_fn_cyw_tcp_listen(i64 %raw_int, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  ret void
}

define void @__omni_trampoline_pack_cyw_tcp_listen(i8* %raw_pack) {
entry:
  %p64 = bitcast i8* %raw_pack to i64*
  %p0 = getelementptr inbounds i64, i64* %p64, i64 0
  %a0 = load i64, i64* %p0
  %p1 = getelementptr inbounds i64, i64* %p64, i64 1
  %a1 = load i64, i64* %p1
  %p2 = getelementptr inbounds i64, i64* %p64, i64 2
  %a2 = load i64, i64* %p2
  %p3 = getelementptr inbounds i64, i64* %p64, i64 3
  %a3 = load i64, i64* %p3
  %p4 = getelementptr inbounds i64, i64* %p64, i64 4
  %a4 = load i64, i64* %p4
  %p5 = getelementptr inbounds i64, i64* %p64, i64 5
  %a5 = load i64, i64* %p5
  call void @free(i8* %raw_pack)
  %res = call i64 @__omni_fn_cyw_tcp_listen(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5)
  ret void
}

define i64 @__omni_fn_cyw_tcp_is_open(i64 %arg0, i64 %arg1, i64 %arg2, i64 %arg3, i64 %arg4, i64 %arg5) {
entry:
  %var_ctx_4361 = alloca i64
  call void @__omni_c_return_ptr(i8* null)
  %fn_ret_4360 = call i64 @__omni_cyw_priv_get_net_ctx(i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  store i64 %fn_ret_4360, i64* %var_ctx_4361
  %t64_4362 = load i64, i64* %var_ctx_4361
  %t_4363 = add i64 %t64_4362, 12
  %ext_i64_4364 = call i64 @mmio_read32(i64 %t_4363)
  %cond_4365 = icmp eq i64 %ext_i64_4364, 4
  br i1 %cond_4365, label %if_then_209, label %if_merge_209

if_then_209:
  ret i64 1
if_merge_209:
  ret i64 0
}


define void @__omni_trampoline_cyw_tcp_is_open(i8* %raw_arg) {
entry:
  %raw_int = ptrtoint i8* %raw_arg to i64
  %res = call i64 @__omni_fn_cyw_tcp_is_open(i64 %raw_int, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  ret void
}

define void @__omni_trampoline_pack_cyw_tcp_is_open(i8* %raw_pack) {
entry:
  %p64 = bitcast i8* %raw_pack to i64*
  %p0 = getelementptr inbounds i64, i64* %p64, i64 0
  %a0 = load i64, i64* %p0
  %p1 = getelementptr inbounds i64, i64* %p64, i64 1
  %a1 = load i64, i64* %p1
  %p2 = getelementptr inbounds i64, i64* %p64, i64 2
  %a2 = load i64, i64* %p2
  %p3 = getelementptr inbounds i64, i64* %p64, i64 3
  %a3 = load i64, i64* %p3
  %p4 = getelementptr inbounds i64, i64* %p64, i64 4
  %a4 = load i64, i64* %p4
  %p5 = getelementptr inbounds i64, i64* %p64, i64 5
  %a5 = load i64, i64* %p5
  call void @free(i8* %raw_pack)
  %res = call i64 @__omni_fn_cyw_tcp_is_open(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5)
  ret void
}

define i64 @__omni_fn_cyw_tcp_send(i64 %arg0, i64 %arg1, i64 %arg2, i64 %arg3, i64 %arg4, i64 %arg5) {
entry:
  %param_msg_ptr_4367 = alloca i64
  %var_ctx_4369 = alloca i64
  %var_n_4376 = alloca i64
  %param_unset_msg_ptr_4366 = icmp eq i64 %arg0, -581005069656919554
  %param_dsel_msg_ptr_4366 = select i1 %param_unset_msg_ptr_4366, i64 0, i64 %arg0
  store i64 %param_dsel_msg_ptr_4366, i64* %param_msg_ptr_4367
  call void @__omni_c_return_ptr(i8* null)
  %fn_ret_4368 = call i64 @__omni_cyw_priv_get_net_ctx(i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  store i64 %fn_ret_4368, i64* %var_ctx_4369
  %t64_4370 = load i64, i64* %var_ctx_4369
  %t_4371 = add i64 %t64_4370, 12
  %ext_i64_4372 = call i64 @mmio_read32(i64 %t_4371)
  %cond_4373 = icmp ne i64 %ext_i64_4372, 4
  br i1 %cond_4373, label %if_then_210, label %if_merge_210

if_then_210:
  ret i64 -1
if_merge_210:
  call void @__omni_c_return_ptr(i8* null)
  %t64_4374 = load i64, i64* %param_msg_ptr_4367
  %fn_ret_4375 = call i64 @__omni_cyw_priv_cstr_len(i64 %t64_4374, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  store i64 %fn_ret_4375, i64* %var_n_4376
  %t64_4377 = load i64, i64* %var_n_4376
  %cond_4378 = icmp sle i64 %t64_4377, 0
  br i1 %cond_4378, label %if_then_211, label %if_merge_211

if_then_211:
  ret i64 0
if_merge_211:
  %t64_4379 = load i64, i64* %var_n_4376
  %cond_4380 = icmp sgt i64 %t64_4379, 1020
  br i1 %cond_4380, label %if_then_212, label %if_merge_212

if_then_212:
  call void @__omni_c_return_ptr(i8* null)
  store i64 1020, i64* %var_n_4376
  br label %if_merge_212

if_merge_212:
  %t64_4381 = load i64, i64* %var_ctx_4369
  %t64_4382 = load i64, i64* %param_msg_ptr_4367
  %t64_4383 = load i64, i64* %var_n_4376
  %fn_ret_4384 = call i64 @__omni_cyw_priv_tcp_send_tracked(i64 %t64_4381, i64 24, i64 %t64_4382, i64 %t64_4383, i64 -581005069656919554, i64 -581005069656919554)
  %t64_4385 = load i64, i64* %var_n_4376
  ret i64 %t64_4385
}


define void @__omni_trampoline_cyw_tcp_send(i8* %raw_arg) {
entry:
  %raw_int = ptrtoint i8* %raw_arg to i64
  %res = call i64 @__omni_fn_cyw_tcp_send(i64 %raw_int, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  ret void
}

define void @__omni_trampoline_pack_cyw_tcp_send(i8* %raw_pack) {
entry:
  %p64 = bitcast i8* %raw_pack to i64*
  %p0 = getelementptr inbounds i64, i64* %p64, i64 0
  %a0 = load i64, i64* %p0
  %p1 = getelementptr inbounds i64, i64* %p64, i64 1
  %a1 = load i64, i64* %p1
  %p2 = getelementptr inbounds i64, i64* %p64, i64 2
  %a2 = load i64, i64* %p2
  %p3 = getelementptr inbounds i64, i64* %p64, i64 3
  %a3 = load i64, i64* %p3
  %p4 = getelementptr inbounds i64, i64* %p64, i64 4
  %a4 = load i64, i64* %p4
  %p5 = getelementptr inbounds i64, i64* %p64, i64 5
  %a5 = load i64, i64* %p5
  call void @free(i8* %raw_pack)
  %res = call i64 @__omni_fn_cyw_tcp_send(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5)
  ret void
}

define i64 @__omni_fn_cyw_tcp_poll_ptr(i64 %arg0, i64 %arg1, i64 %arg2, i64 %arg3, i64 %arg4, i64 %arg5) {
entry:
  %fn_ret_4386 = call i64 @__omni_fn_cyw_ws_poll_ptr(i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  ret i64 %fn_ret_4386
}


define void @__omni_trampoline_cyw_tcp_poll_ptr(i8* %raw_arg) {
entry:
  %raw_int = ptrtoint i8* %raw_arg to i64
  %res = call i64 @__omni_fn_cyw_tcp_poll_ptr(i64 %raw_int, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  ret void
}

define void @__omni_trampoline_pack_cyw_tcp_poll_ptr(i8* %raw_pack) {
entry:
  %p64 = bitcast i8* %raw_pack to i64*
  %p0 = getelementptr inbounds i64, i64* %p64, i64 0
  %a0 = load i64, i64* %p0
  %p1 = getelementptr inbounds i64, i64* %p64, i64 1
  %a1 = load i64, i64* %p1
  %p2 = getelementptr inbounds i64, i64* %p64, i64 2
  %a2 = load i64, i64* %p2
  %p3 = getelementptr inbounds i64, i64* %p64, i64 3
  %a3 = load i64, i64* %p3
  %p4 = getelementptr inbounds i64, i64* %p64, i64 4
  %a4 = load i64, i64* %p4
  %p5 = getelementptr inbounds i64, i64* %p64, i64 5
  %a5 = load i64, i64* %p5
  call void @free(i8* %raw_pack)
  %res = call i64 @__omni_fn_cyw_tcp_poll_ptr(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5)
  ret void
}

define i64 @__omni_fn_cyw_tcp_close(i64 %arg0, i64 %arg1, i64 %arg2, i64 %arg3, i64 %arg4, i64 %arg5) {
entry:
  %var_ctx_4388 = alloca i64
  call void @__omni_c_return_ptr(i8* null)
  %fn_ret_4387 = call i64 @__omni_cyw_priv_get_net_ctx(i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  store i64 %fn_ret_4387, i64* %var_ctx_4388
  %t64_4389 = load i64, i64* %var_ctx_4388
  %t_4390 = add i64 %t64_4389, 12
  %ext_i64_4391 = call i64 @mmio_read32(i64 %t_4390)
  %cond_4392 = icmp sge i64 %ext_i64_4391, 2
  br i1 %cond_4392, label %if_then_213, label %if_merge_213

if_then_213:
  %t64_4393 = load i64, i64* %var_ctx_4388
  %fn_ret_4394 = call i64 @__omni_cyw_priv_tcp_send_tracked(i64 %t64_4393, i64 17, i64 0, i64 0, i64 -581005069656919554, i64 -581005069656919554)
  %t64_4395 = load i64, i64* %var_ctx_4388
  %t_4396 = add i64 %t64_4395, 12
  %ext_i64_4397 = call i64 @mmio_write32(i64 %t_4396, i64 1)
  %t64_4398 = load i64, i64* %var_ctx_4388
  %t_4399 = add i64 %t64_4398, 100
  %ext_i64_4400 = call i64 @mmio_write32(i64 %t_4399, i64 0)
  %t64_4401 = load i64, i64* %var_ctx_4388
  %t_4402 = add i64 %t64_4401, 116
  %ext_i64_4403 = call i64 @mmio_write32(i64 %t_4402, i64 0)
  %t64_4404 = load i64, i64* %var_ctx_4388
  %t_4405 = add i64 %t64_4404, 120
  %ext_i64_4406 = call i64 @mmio_write32(i64 %t_4405, i64 0)
  br label %if_merge_213

if_merge_213:
  ret i64 0
}


define void @__omni_trampoline_cyw_tcp_close(i8* %raw_arg) {
entry:
  %raw_int = ptrtoint i8* %raw_arg to i64
  %res = call i64 @__omni_fn_cyw_tcp_close(i64 %raw_int, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  ret void
}

define void @__omni_trampoline_pack_cyw_tcp_close(i8* %raw_pack) {
entry:
  %p64 = bitcast i8* %raw_pack to i64*
  %p0 = getelementptr inbounds i64, i64* %p64, i64 0
  %a0 = load i64, i64* %p0
  %p1 = getelementptr inbounds i64, i64* %p64, i64 1
  %a1 = load i64, i64* %p1
  %p2 = getelementptr inbounds i64, i64* %p64, i64 2
  %a2 = load i64, i64* %p2
  %p3 = getelementptr inbounds i64, i64* %p64, i64 3
  %a3 = load i64, i64* %p3
  %p4 = getelementptr inbounds i64, i64* %p64, i64 4
  %a4 = load i64, i64* %p4
  %p5 = getelementptr inbounds i64, i64* %p64, i64 5
  %a5 = load i64, i64* %p5
  call void @free(i8* %raw_pack)
  %res = call i64 @__omni_fn_cyw_tcp_close(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5)
  ret void
}

define i64 @__omni_fn_cyw_ws_is_open(i64 %arg0, i64 %arg1, i64 %arg2, i64 %arg3, i64 %arg4, i64 %arg5) {
entry:
  %var_ctx_4408 = alloca i64
  call void @__omni_c_return_ptr(i8* null)
  %fn_ret_4407 = call i64 @__omni_cyw_priv_get_net_ctx(i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  store i64 %fn_ret_4407, i64* %var_ctx_4408
  %t64_4409 = load i64, i64* %var_ctx_4408
  %t_4410 = add i64 %t64_4409, 12
  %ext_i64_4411 = call i64 @mmio_read32(i64 %t_4410)
  %cond_4412 = icmp eq i64 %ext_i64_4411, 4
  br i1 %cond_4412, label %if_then_214, label %if_merge_214

if_then_214:
  ret i64 1
if_merge_214:
  ret i64 0
}


define void @__omni_trampoline_cyw_ws_is_open(i8* %raw_arg) {
entry:
  %raw_int = ptrtoint i8* %raw_arg to i64
  %res = call i64 @__omni_fn_cyw_ws_is_open(i64 %raw_int, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  ret void
}

define void @__omni_trampoline_pack_cyw_ws_is_open(i8* %raw_pack) {
entry:
  %p64 = bitcast i8* %raw_pack to i64*
  %p0 = getelementptr inbounds i64, i64* %p64, i64 0
  %a0 = load i64, i64* %p0
  %p1 = getelementptr inbounds i64, i64* %p64, i64 1
  %a1 = load i64, i64* %p1
  %p2 = getelementptr inbounds i64, i64* %p64, i64 2
  %a2 = load i64, i64* %p2
  %p3 = getelementptr inbounds i64, i64* %p64, i64 3
  %a3 = load i64, i64* %p3
  %p4 = getelementptr inbounds i64, i64* %p64, i64 4
  %a4 = load i64, i64* %p4
  %p5 = getelementptr inbounds i64, i64* %p64, i64 5
  %a5 = load i64, i64* %p5
  call void @free(i8* %raw_pack)
  %res = call i64 @__omni_fn_cyw_ws_is_open(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5)
  ret void
}

define i64 @__omni_fn_cyw_ws_send(i64 %arg0, i64 %arg1, i64 %arg2, i64 %arg3, i64 %arg4, i64 %arg5) {
entry:
  %param_msg_ptr_4414 = alloca i64
  %var_ctx_4416 = alloca i64
  %var_n_4423 = alloca i64
  %param_unset_msg_ptr_4413 = icmp eq i64 %arg0, -581005069656919554
  %param_dsel_msg_ptr_4413 = select i1 %param_unset_msg_ptr_4413, i64 0, i64 %arg0
  store i64 %param_dsel_msg_ptr_4413, i64* %param_msg_ptr_4414
  call void @__omni_c_return_ptr(i8* null)
  %fn_ret_4415 = call i64 @__omni_cyw_priv_get_net_ctx(i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  store i64 %fn_ret_4415, i64* %var_ctx_4416
  %t64_4417 = load i64, i64* %var_ctx_4416
  %t_4418 = add i64 %t64_4417, 12
  %ext_i64_4419 = call i64 @mmio_read32(i64 %t_4418)
  %cond_4420 = icmp ne i64 %ext_i64_4419, 4
  br i1 %cond_4420, label %if_then_215, label %if_merge_215

if_then_215:
  ret i64 -1
if_merge_215:
  call void @__omni_c_return_ptr(i8* null)
  %t64_4421 = load i64, i64* %param_msg_ptr_4414
  %fn_ret_4422 = call i64 @__omni_cyw_priv_cstr_len(i64 %t64_4421, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  store i64 %fn_ret_4422, i64* %var_n_4423
  %t64_4424 = load i64, i64* %var_ctx_4416
  %t64_4425 = load i64, i64* %param_msg_ptr_4414
  %t64_4426 = load i64, i64* %var_n_4423
  %fn_ret_4427 = call i64 @__omni_cyw_priv_ws_send_raw_frame(i64 %t64_4424, i64 1, i64 %t64_4425, i64 %t64_4426, i64 -581005069656919554, i64 -581005069656919554)
  ret i64 %fn_ret_4427
}


define void @__omni_trampoline_cyw_ws_send(i8* %raw_arg) {
entry:
  %raw_int = ptrtoint i8* %raw_arg to i64
  %res = call i64 @__omni_fn_cyw_ws_send(i64 %raw_int, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  ret void
}

define void @__omni_trampoline_pack_cyw_ws_send(i8* %raw_pack) {
entry:
  %p64 = bitcast i8* %raw_pack to i64*
  %p0 = getelementptr inbounds i64, i64* %p64, i64 0
  %a0 = load i64, i64* %p0
  %p1 = getelementptr inbounds i64, i64* %p64, i64 1
  %a1 = load i64, i64* %p1
  %p2 = getelementptr inbounds i64, i64* %p64, i64 2
  %a2 = load i64, i64* %p2
  %p3 = getelementptr inbounds i64, i64* %p64, i64 3
  %a3 = load i64, i64* %p3
  %p4 = getelementptr inbounds i64, i64* %p64, i64 4
  %a4 = load i64, i64* %p4
  %p5 = getelementptr inbounds i64, i64* %p64, i64 5
  %a5 = load i64, i64* %p5
  call void @free(i8* %raw_pack)
  %res = call i64 @__omni_fn_cyw_ws_send(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5)
  ret void
}

define i64 @__omni_fn_cyw_ws_poll_ptr(i64 %arg0, i64 %arg1, i64 %arg2, i64 %arg3, i64 %arg4, i64 %arg5) {
entry:
  %var_ctx_4429 = alloca i64
  %var_loops_4433 = alloca i64
  %var_cur_ms_4451 = alloca i64
  %var_unack_len_4455 = alloca i64
  %var_unack_flags_4459 = alloca i64
  %var_dt_4479 = alloca i64
  %var_retries_4486 = alloca i64
  %var_bdt_4538 = alloca i64
  call void @__omni_c_return_ptr(i8* null)
  %fn_ret_4428 = call i64 @__omni_cyw_priv_get_net_ctx(i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  store i64 %fn_ret_4428, i64* %var_ctx_4429
  %t64_4430 = load i64, i64* %var_ctx_4429
  %t_4431 = add i64 %t64_4430, 120
  %ext_i64_4432 = call i64 @mmio_write32(i64 %t_4431, i64 0)
  call void @__omni_c_return_ptr(i8* null)
  store i64 0, i64* %var_loops_4433
  br label %while_cond_216

while_cond_216:
  %t64_4434 = load i64, i64* %var_loops_4433
  %cond_4435 = icmp slt i64 %t64_4434, 6
  br i1 %cond_4435, label %while_body_216, label %while_exit_216

while_body_216:
  %t64_4436 = load i64, i64* %var_ctx_4429
  %fn_ret_4437 = call i64 @__omni_fn_cyw_poll_one_pkt(i64 %t64_4436, i64 0, i64 0, i64 0, i64 -581005069656919554, i64 -581005069656919554)
  %cond_4438 = icmp eq i64 %fn_ret_4437, 0
  br i1 %cond_4438, label %if_then_217, label %if_merge_217

if_then_217:
  br label %while_exit_216
if_merge_217:
  %t64_4439 = load i64, i64* %var_ctx_4429
  %t_4440 = add i64 %t64_4439, 120
  %ext_i64_4441 = call i64 @mmio_read32(i64 %t_4440)
  %cond_4442 = icmp ne i64 %ext_i64_4441, 0
  br i1 %cond_4442, label %if_then_218, label %if_merge_218

if_then_218:
  %t64_4443 = load i64, i64* %var_ctx_4429
  %t_4444 = add i64 %t64_4443, 120
  %ext_i64_4445 = call i64 @mmio_write32(i64 %t_4444, i64 0)
  %t64_4446 = load i64, i64* %var_ctx_4429
  %t_4447 = add i64 %t64_4446, 1184
  ret i64 %t_4447
if_merge_218:
  call void @__omni_c_return_ptr(i8* null)
  %t64_4448 = load i64, i64* %var_loops_4433
  %t_4449 = add i64 %t64_4448, 1
  store i64 %t_4449, i64* %var_loops_4433
  br label %while_cond_216

while_exit_216:
  call void @__omni_c_return_ptr(i8* null)
  %fn_ret_4450 = call i64 @__omni_cyw_priv_now_ms(i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  store i64 %fn_ret_4450, i64* %var_cur_ms_4451
  call void @__omni_c_return_ptr(i8* null)
  %t64_4452 = load i64, i64* %var_ctx_4429
  %t_4453 = add i64 %t64_4452, 100
  %ext_i64_4454 = call i64 @mmio_read32(i64 %t_4453)
  store i64 %ext_i64_4454, i64* %var_unack_len_4455
  call void @__omni_c_return_ptr(i8* null)
  %t64_4456 = load i64, i64* %var_ctx_4429
  %t_4457 = add i64 %t64_4456, 104
  %ext_i64_4458 = call i64 @mmio_read32(i64 %t_4457)
  store i64 %ext_i64_4458, i64* %var_unack_flags_4459
  %t64_4461 = load i64, i64* %var_ctx_4429
  %t_4462 = add i64 %t64_4461, 12
  %ext_i64_4463 = call i64 @mmio_read32(i64 %t_4462)
  %cond_4464 = icmp sge i64 %ext_i64_4463, 2
  br i1 %cond_4464, label %sc_and_rhs_4460, label %sc_and_short_4460

sc_and_short_4460:
  br label %sc_and_merge_4460

sc_and_rhs_4460:
  %t64_4466 = load i64, i64* %var_unack_len_4455
  %cond_4467 = icmp sgt i64 %t64_4466, 0
  br i1 %cond_4467, label %sc_or_short_4465, label %sc_or_rhs_4465

sc_or_short_4465:
  br label %sc_or_merge_4465

sc_or_rhs_4465:
  %t64_4468 = load i64, i64* %var_unack_flags_4459
  %t_4469 = and i64 %t64_4468, 3
  %cond_4470 = icmp ne i64 %t_4469, 0
  br label %sc_or_done_4465

sc_or_done_4465:
  br label %sc_or_merge_4465

sc_or_merge_4465:
  %or_cond_4471 = phi i1 [ true, %sc_or_short_4465 ], [ %cond_4470, %sc_or_done_4465 ]
  br label %sc_and_done_4460

sc_and_done_4460:
  br label %sc_and_merge_4460

sc_and_merge_4460:
  %and_cond_4472 = phi i1 [ false, %sc_and_short_4460 ], [ %or_cond_4471, %sc_and_done_4460 ]
  br i1 %and_cond_4472, label %if_then_219, label %if_merge_219

if_then_219:
  call void @__omni_c_return_ptr(i8* null)
  %t64_4473 = load i64, i64* %var_cur_ms_4451
  %t64_4474 = load i64, i64* %var_ctx_4429
  %t_4475 = add i64 %t64_4474, 108
  %ext_i64_4476 = call i64 @mmio_read32(i64 %t_4475)
  %t_4477 = sub i64 %t64_4473, %ext_i64_4476
  %t_4478 = and i64 %t_4477, 2147483647
  store i64 %t_4478, i64* %var_dt_4479
  %t64_4480 = load i64, i64* %var_dt_4479
  %cond_4481 = icmp sge i64 %t64_4480, 300
  br i1 %cond_4481, label %if_then_220, label %if_merge_220

if_then_220:
  call void @__omni_c_return_ptr(i8* null)
  %t64_4482 = load i64, i64* %var_ctx_4429
  %t_4483 = add i64 %t64_4482, 112
  %ext_i64_4484 = call i64 @mmio_read32(i64 %t_4483)
  %t_4485 = add i64 %ext_i64_4484, 1
  store i64 %t_4485, i64* %var_retries_4486
  %t64_4487 = load i64, i64* %var_retries_4486
  %cond_4488 = icmp sgt i64 %t64_4487, 8
  br i1 %cond_4488, label %if_then_221, label %if_else_221

if_then_221:
  %t64_4489 = load i64, i64* %var_ctx_4429
  %t_4490 = add i64 %t64_4489, 12
  %ext_i64_4491 = call i64 @mmio_write32(i64 %t_4490, i64 1)
  %t64_4492 = load i64, i64* %var_ctx_4429
  %t_4493 = add i64 %t64_4492, 100
  %ext_i64_4494 = call i64 @mmio_write32(i64 %t_4493, i64 0)
  %t64_4495 = load i64, i64* %var_ctx_4429
  %t_4496 = add i64 %t64_4495, 104
  %ext_i64_4497 = call i64 @mmio_write32(i64 %t_4496, i64 0)
  br label %if_merge_221

if_else_221:
  %t64_4498 = load i64, i64* %var_ctx_4429
  %t_4499 = add i64 %t64_4498, 112
  %t64_4500 = load i64, i64* %var_retries_4486
  %ext_i64_4501 = call i64 @mmio_write32(i64 %t_4499, i64 %t64_4500)
  %t64_4502 = load i64, i64* %var_ctx_4429
  %t_4503 = add i64 %t64_4502, 108
  %t64_4504 = load i64, i64* %var_cur_ms_4451
  %ext_i64_4505 = call i64 @mmio_write32(i64 %t_4503, i64 %t64_4504)
  %t64_4506 = load i64, i64* %var_ctx_4429
  %t64_4507 = load i64, i64* %var_ctx_4429
  %t_4508 = add i64 %t64_4507, 72
  %t64_4509 = load i64, i64* %var_ctx_4429
  %t_4510 = add i64 %t64_4509, 80
  %t64_4511 = load i64, i64* %var_ctx_4429
  %t_4512 = add i64 %t64_4511, 84
  %ext_i64_4513 = call i64 @mmio_read32(i64 %t_4512)
  %t64_4514 = load i64, i64* %var_ctx_4429
  %t_4515 = add i64 %t64_4514, 16
  %ext_i64_4516 = call i64 @mmio_read32(i64 %t_4515)
  %t64_4517 = load i64, i64* %var_ctx_4429
  %t_4518 = add i64 %t64_4517, 96
  %ext_i64_4519 = call i64 @mmio_read32(i64 %t_4518)
  %t64_4520 = load i64, i64* %var_ctx_4429
  %t_4521 = add i64 %t64_4520, 92
  %ext_i64_4522 = call i64 @mmio_read32(i64 %t_4521)
  %t64_4523 = load i64, i64* %var_unack_flags_4459
  %t64_4524 = load i64, i64* %var_ctx_4429
  %t_4525 = add i64 %t64_4524, 6464
  %t64_4526 = load i64, i64* %var_unack_len_4455
  %fn_ret_4527 = call i64 @__omni_cyw_priv_tcp_send_segment(i64 %t64_4506, i64 %t_4508, i64 %t_4510, i64 %ext_i64_4513, i64 %ext_i64_4516, i64 %ext_i64_4519, i64 %ext_i64_4522, i64 %t64_4523, i64 %t_4525, i64 %t64_4526)
  br label %if_merge_221

if_merge_221:
  br label %if_merge_220

if_merge_220:
  br label %if_merge_219

if_merge_219:
  %t64_4528 = load i64, i64* %var_ctx_4429
  %t_4529 = add i64 %t64_4528, 8
  %ext_i64_4530 = call i64 @mmio_read32(i64 %t_4529)
  %cond_4531 = icmp eq i64 %ext_i64_4530, 4
  br i1 %cond_4531, label %if_then_222, label %if_merge_222

if_then_222:
  call void @__omni_c_return_ptr(i8* null)
  %t64_4532 = load i64, i64* %var_cur_ms_4451
  %t64_4533 = load i64, i64* %var_ctx_4429
  %t_4534 = add i64 %t64_4533, 28
  %ext_i64_4535 = call i64 @mmio_read32(i64 %t_4534)
  %t_4536 = sub i64 %t64_4532, %ext_i64_4535
  %t_4537 = and i64 %t_4536, 2147483647
  store i64 %t_4537, i64* %var_bdt_4538
  %t64_4539 = load i64, i64* %var_bdt_4538
  %cond_4540 = icmp sge i64 %t64_4539, 2000
  br i1 %cond_4540, label %if_then_223, label %if_merge_223

if_then_223:
  %t64_4541 = load i64, i64* %var_ctx_4429
  %t_4542 = add i64 %t64_4541, 28
  %t64_4543 = load i64, i64* %var_cur_ms_4451
  %ext_i64_4544 = call i64 @mmio_write32(i64 %t_4542, i64 %t64_4543)
  %t64_4545 = load i64, i64* %var_ctx_4429
  %fn_ret_4546 = call i64 @__omni_cyw_priv_broadcast_discovery(i64 %t64_4545, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  br label %if_merge_223

if_merge_223:
  br label %if_merge_222

if_merge_222:
  %t64_4547 = load i64, i64* %var_ctx_4429
  %t_4548 = add i64 %t64_4547, 156
  ret i64 %t_4548
}


define void @__omni_trampoline_cyw_ws_poll_ptr(i8* %raw_arg) {
entry:
  %raw_int = ptrtoint i8* %raw_arg to i64
  %res = call i64 @__omni_fn_cyw_ws_poll_ptr(i64 %raw_int, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554, i64 -581005069656919554)
  ret void
}

define void @__omni_trampoline_pack_cyw_ws_poll_ptr(i8* %raw_pack) {
entry:
  %p64 = bitcast i8* %raw_pack to i64*
  %p0 = getelementptr inbounds i64, i64* %p64, i64 0
  %a0 = load i64, i64* %p0
  %p1 = getelementptr inbounds i64, i64* %p64, i64 1
  %a1 = load i64, i64* %p1
  %p2 = getelementptr inbounds i64, i64* %p64, i64 2
  %a2 = load i64, i64* %p2
  %p3 = getelementptr inbounds i64, i64* %p64, i64 3
  %a3 = load i64, i64* %p3
  %p4 = getelementptr inbounds i64, i64* %p64, i64 4
  %a4 = load i64, i64* %p4
  %p5 = getelementptr inbounds i64, i64* %p64, i64 5
  %a5 = load i64, i64* %p5
  call void @free(i8* %raw_pack)
  %res = call i64 @__omni_fn_cyw_ws_poll_ptr(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5)
  ret void
}

define i32 @__omni_cyw_priv_main(i8* %data, i64 %len) {
entry:
  ret i32 0
}


define void @__omni_trampoline_main(i8* %raw_arg) {
entry:
  %res = call i32 @__omni_cyw_priv_main(i8* %raw_arg, i64 0)
  ret void
}

