// webpack module 120048 from _next/static/chunks/pages/_app-67b4d2c87c90572b.js
webpackModule(120048, (g, b, y) => {
  "use strict";

  y.d(b, {
    $D: () => e5,
    $u: () => e$,
    A3: () => $,
    AA: () => eY,
    AU: () => eN,
    C3: () => ed,
    CB: () => eR,
    Eo: () => el,
    Ez: () => eM,
    FM: () => Y,
    Fz: () => e8,
    GZ: () => eT,
    HU: () => et,
    Hw: () => e7,
    Iz: () => eh,
    JT: () => ea,
    KZ: () => X,
    L: () => e0,
    LA: () => eS,
    LF: () => P,
    LW: () => eg,
    Md: () => ee,
    N3: () => ef,
    NW: () => ec,
    O4: () => ex,
    OO: () => e4,
    Oi: () => K,
    QH: () => ey,
    Qr: () => ew,
    Rj: () => q,
    TX: () => Z,
    Th: () => e6,
    V: () => ei,
    Xf: () => en,
    YY: () => M,
    Z$: () => e_,
    ZI: () => em,
    _C: () => eF,
    aO: () => ek,
    bO: () => U,
    cm: () => W,
    d3: () => eP,
    dB: () => er,
    dI: () => eA,
    fT: () => eo,
    gg: () => e1,
    hU: () => N,
    hd: () => eI,
    id: () => eb,
    l_: () => j,
    nG: () => eB,
    nn: () => B,
    p2: () => es,
    p4: () => eC,
    pZ: () => z,
    pc: () => eu,
    qE: () => eE,
    qf: () => Q,
    rQ: () => R,
    sN: () => eU,
    tY: () => e2,
    td: () => ev
  });
  var _,
    x,
    v,
    w = y(718895),
    A = y(549213),
    k = y(165523),
    S = y(777473),
    C = y(214472),
    T = y(454358),
    I = y(80126),
    E = y(378733);
  let N = g => C.Ay.post({
      url: "/api/v1/mutil_window_token_info",
      data: g,
      authType: w.AuthType.Access,
      enforeRequest: !0,
      extractData: !1
    }),
    M = g => E.j.get({
      url: `/api/v1/mutil_window_token_link_rug_vote/${g.chain}/${g.address}`
    }),
    P = (g = {}) => (0, T.fi)({
      url: "/api/v1/token_prices",
      params: g
    }),
    R = g => C.Ay.post({
      url: "/api/v1/token_youtube_published_at",
      data: g
    });
  var B = ((_ = {}).Meteora_virtual_curve = "meteora_virtual_curve", _.Meteora_damm_v2 = "meteora_damm_v2", _.Fourmeme = "bn_fourmeme", _.Virtuals = "virtuals", _.Clanker = "clanker", _.Bankr = "bankr", _);
  let j = (g, b, y = {}) => (0, T.pX)({
      url: `/api/v1/token_pool_fee_info/${g}/${b}`,
      params: {
        ...y,
        needToken: !0
      }
    }),
    O = g => "object" == typeof g && null !== g,
    D = g => "number" == typeof g && Number.isFinite(g),
    L = g => "string" == typeof g && g.trim().length > 0,
    F = g => {
      if (!O(g)) return null;
      let b = Array.isArray(g.top_receivers) ? g.top_receivers.flatMap(g => O(g) && L(g.wallet) ? [{
          wallet: g.wallet,
          ...(L(g.received) ? {
            received: g.received
          } : {}),
          ...(L(g.received_raw) ? {
            received_raw: g.received_raw
          } : {})
        }] : []) : void 0,
        y = {
          ...(D(g.holder_reward_fee_bps) ? {
            holder_reward_fee_bps: g.holder_reward_fee_bps
          } : {}),
          ...(L(g.quote_mint) ? {
            quote_mint: g.quote_mint
          } : {}),
          ...(D(g.quote_decimals) ? {
            quote_decimals: g.quote_decimals
          } : {}),
          ...(L(g.total_distributed) ? {
            total_distributed: g.total_distributed
          } : {}),
          ...(L(g.total_distributed_raw) ? {
            total_distributed_raw: g.total_distributed_raw
          } : {}),
          ...(D(g.payouts) ? {
            payouts: g.payouts
          } : {}),
          ...(D(g.last_payout_at) ? {
            last_payout_at: g.last_payout_at
          } : {}),
          ...(b ? {
            top_receivers: b
          } : {})
        };
      return Object.keys(y).length > 0 ? y : null;
    },
    U = async (g, b) => {
      let y = await (0, T.pX)({
        url: `/api/v1/token_fee_info/${g}/${b}`
      });
      return {
        ...y,
        holder_rewards: F(y.holder_rewards)
      };
    },
    $ = (g, b) => (0, T.pX)({
      url: `/api/v1/token_dev_info/${g}/${b}`
    }),
    W = (g, b) => {
      let y;
      if (g === S.lg.Sol) y = "/api/v1/token_security_sol";else if (g === S.lg.Tron) y = "/defi/quotation/v1/tokens/security";else if (k.NK.has(g) && "evm" === k.NK.get(g).type) y = "/api/v1/token_security_evm";else throw Error(`No token security path for chain: ${g}`);
      return (0, T.pX)({
        url: `${y}/${g}/${b}`
      });
    },
    q = (g, b) => (0, C.jJ)({
      url: `/api/v1/crypto_kline/crypto/${g}`,
      data: b
    }),
    K = (g, b, y) => E.j.get({
      url: `/api/v1/token_candles/${g}/${b}`,
      params: y
    }),
    z = (g, b, y) => E.j.get({
      url: `/api/v1/token_mcap_candles/${g}/${b}`,
      params: y
    }),
    V = (g, b, y, _ = "", x) => {
      let v = `/vas/api/v1/token_trades/${g}/${b}${_}`;
      return (0, T.pX)({
        url: v,
        params: y,
        options: x
      });
    },
    H = (g, b, y, _ = "", x) => (0, T.pX)({
      url: `/vas/api/v1/follow/token_trades/${g}/${b}${_}`,
      params: y,
      options: x
    }),
    G = (g, b, y, _ = "", x) => (0, T.pX)({
      url: `${(0, w.globalConfig)().hostConfig.vasApi}/remark/token_trades/${g}/${b}${_}`,
      params: y,
      options: x
    }),
    Q = (g, b, y, _ = "", x) => {
      y.limit = 50;
      let v = V;
      return y.following ? v = H : y.remarks && (v = G), v(g, b, y, _, x);
    },
    J = (g, b, y, _ = "", x) => {
      let v = `/vas/api/mul-region/token_trades_v2/${g}/${b}${_}`;
      return (0, T.pX)({
        url: v,
        params: y,
        options: x
      });
    },
    Y = (g, b, y, _ = "", x) => {
      y.limit = 50;
      let v = J;
      return y.following ? v = H : y.remarks && (v = G), v(g, b, y, _, x);
    },
    X = (g, b, y, _ = "") => C.Ay.get({
      url: `/vas/api/v1/follow/agged_token_trades/${g}/${b}${_}`,
      data: y,
      authType: w.AuthType.Access
    }),
    Z = (g, b, y, _ = "") => C.Ay.get({
      url: `${(0, w.globalConfig)().hostConfig.vasApi}/remark/agged_token_trades/${g}/${b}${_}`,
      data: y,
      authType: w.AuthType.Access
    }),
    ee = (g, b, y, _ = "") => C.Ay.get({
      url: `/api/v1/tg_calls/klines/${g}/${b}${_}`,
      data: y
    }),
    et = (g, b, y, _ = "") => C.Ay.get({
      url: `/api/v1/discord_calls/klines/${g}/${b}${_}`,
      data: y
    }),
    er = (g, b) => (0, T.pX)({
      url: `/api/v1/tokens/top_buyers/${g}/${b}`
    }),
    en = (g, b) => (0, T.pX)({
      url: `${(0, w.globalConfig)().hostConfig.vasApi}/token_watched_wallet_stat/${g}/${b}`,
      params: {
        needToken: !0
      }
    }),
    ei = (g, b) => (0, T.pX)({
      url: `/api/v1/token_liquidity_stats/${g}/${b}`,
      params: {
        needToken: !0
      }
    }),
    ea = (g, b) => (0, T.pX)({
      url: `/api/v1/token_stat/${g || "eth"}/${b}`
    }),
    eo = (g, b, y = {}) => (0, T.pX)({
      url: `/vas/api/v1/token_trader_stat/${g || "eth"}/${b}`,
      params: y
    }),
    es = (g, b, y = {}, _) => (0, T.pX)({
      url: `/vas/api/v1/token_holder_stat/${g || "eth"}/${b}`,
      params: y,
      axiosRequestConfig: _
    }),
    el = (g, b) => (0, T.pX)({
      url: `/api/v1/tokens/rug_history/${g || "eth"}/${b}`
    });
  var ec = ((x = {}).avg_holding_balance = "avg_holding_balance", x.holder_count = "holder_count", x.top10_holder_percent = "top10_holder_percent", x.top100_holder_percent = "top100_holder_percent", x.bundler_percent = "bundler_percent", x.insider_percent = "insider_percent", x.bot_degen_percent = "bot_degen_percent", x.entrapment_percent = "entrapment_percent", x);
  let eu = (g, b, y, _ = !1) => {
      let x = new URLSearchParams();
      return y.forEach(g => x.append("trends_type", g)), _ && x.append("with_fine_grained", "true"), (0, T.pX)({
        url: `/api/v1/token_trends/${g}/${b}?${x.toString()}`
      });
    },
    ed = (g, b) => (0, T.pX)({
      url: `/vas/api/v1/token_liquidity_trend/${g}/${b}`
    });
  function ep(g) {
    return null == g ? g : -1 === g ? null : Number(g);
  }
  function eh(g, b) {
    return g === S.lg.Sol ? {
      ...b,
      renounced_mint: ep(b.renounced_mint),
      renounced_freeze_account: ep(b.renounced_freeze_account),
      top_10_holder_rate: ep(b.top_10_holder_rate),
      burn_ratio: ep(b.burn_ratio),
      dev_token_burn_amount: ep(b.dev_token_burn_amount),
      dev_token_burn_ratio: ep(b.dev_token_burn_ratio)
    } : {
      ...b,
      renounced_mint: ep(b.renounced_mint),
      renounced_freeze_account: ep(b.renounced_freeze_account),
      dev_token_burn_amount: ep(b.dev_token_burn_amount),
      dev_token_burn_ratio: ep(b.dev_token_burn_ratio),
      burn_ratio: ep(b.burn_ratio),
      buy_tax: ep(b.buy_tax),
      is_honeypot: ep(b.is_honeypot),
      is_open_source: ep(b.is_open_source),
      sell_tax: ep(b.sell_tax),
      renounced: ep(b.is_renounced),
      honeypot_data: {
        holderAnalysis: {
          successful: b.can_sell,
          failed: b.can_not_sell,
          averageTax: b.average_tax,
          highestTax: b.high_tax
        }
      },
      lockInfo: g === S.lg.Tron ? b.lockInfo : (() => {
        let {
          burnRatio: g,
          lockRatio: y,
          totalLockRatio: _
        } = (0, I.QD)(b.lock_summary?.lock_detail);
        return {
          lockTag: b.lock_summary?.lock_tags,
          isLock: b.lock_summary?.is_locked,
          lockDetail: b.lock_summary?.lock_detail?.map(g => ({
            percent: g.percent,
            pool: g.pool,
            isBlackHole: g.is_blackhole
          })),
          lockPercent: ep(b.lock_summary?.lock_percent),
          leftLockPercent: ep(b.lock_summary?.left_lock_percent),
          burnRatio: g,
          lockRatio: y,
          totalLockRatio: _
        };
      })(),
      top_10_holder_rate: ep(b.top_10_holder_rate)
    };
  }
  let ef = g => {
      let {
        chain: b,
        address: y,
        order_by: _,
        direction: x,
        migrate_state: v
      } = g;
      return C.Ay.get({
        url: `/api/v1/dev_created_tokens/${b}/${y}`,
        data: {
          ...(_ && {
            order_by: _
          }),
          ...(x && {
            direction: x
          }),
          migrate_state: v
        }
      });
    },
    em = async g => {
      let {
          address: b,
          order_by: y,
          direction: _,
          migrate_state: x
        } = g,
        v = await (0, A._)(C.Ay.get({
          url: `/api/v1/dev_created_tokens_multi_chain/${b}`,
          data: {
            ...(y && {
              order_by: y
            }),
            ...(_ && {
              direction: _
            }),
            ...(x && {
              migrate_state: x
            })
          }
        }));
      if (!Array.isArray(v?.tokens)) throw Error("dev_created_tokens_multi_chain: 响应缺少 tokens 数组");
      return v;
    },
    eg = g => C.Ay.get({
      url: "/vas/api/v1/similar_coin_max_and_earliest",
      data: g
    }),
    eb = g => C.Ay.get({
      url: "/vas/api/v1/similar_coin",
      data: g
    }),
    ey = g => {
      let {
        chain: b,
        token_address: y
      } = g;
      return C.Ay.get({
        url: `/api/v1/website_info/${b}/${y}`,
        data: g
      });
    },
    e_ = ({
      chain: g,
      wallet_address: b
    }) => C.Ay.get({
      url: `/api/v1/native_transfer/${g}/${b}`,
      data: {
        chain: g,
        wallet_address: b
      }
    }),
    ex = g => (0, A._)(C.Ay.post({
      url: "/api/v1/logo/logo_dup_detail",
      data: g
    })),
    ev = (g, b) => C.Ay.post({
      url: "/api/v1/token_info_brief_multi_chain",
      data: {
        addresses: [g],
        chain: b
      }
    });
  function ew(g, b) {
    return C.Ay.post({
      url: "/api/v1/token_info_brief",
      data: {
        chain: g,
        addresses: b
      }
    });
  }
  function eA(g, b) {
    return C.Ay.post({
      url: "/api/v1/meme_quote_info",
      data: {
        chain: g,
        addresses: b
      }
    });
  }
  let ek = (g, b) => (0, A._)(C.Ay.get({
    url: `/api/v1/token_bundler_stat/${g}/${b}`
  }));
  var eS = ((v = {}).SPLIT = "split", v.GIFT = "gift", v.RWA = "rwa", v);
  let eC = (g, b, y) => (0, A._)(C.Ay.post({
      url: g ? "/tapi/v1/create_token" : `/xapi/v1/${b}/create_token_route`,
      data: y,
      authType: w.AuthType.Access,
      timeout: 12e4
    })),
    eT = g => (0, A._)(C.Ay.post({
      url: "/xapi/v1/sol/create_token/preview",
      data: g,
      authType: w.AuthType.Access
    })),
    eI = (g, b, y) => (0, A._)(C.Ay.post({
      url: g ? "/tapi/v1/submit_tx" : `/xapi/v1/${b}/submit_tx`,
      data: y,
      authType: w.AuthType.Access
    })),
    eE = (g, b, y, _, x) => (0, A._)(C.Ay.post({
      url: `/xapi/v1/${g}/query_meme_param`,
      data: {
        user_prompt: b,
        desc_info: y,
        need_icon: _,
        need_text: x
      },
      timeout: 2e4
    })),
    eN = (g, b) => {
      let y = new FormData();
      if (b.userPrompt && y.append("user_prompt", b.userPrompt), b.imageUrls?.length) for (let g of b.imageUrls.slice(0, 3)) y.append("image_urls", g);
      return (0, A._)(C.Ay.post({
        url: `/xapi/v1/${g}/query_meme_param2`,
        data: y,
        headers: {
          "Content-Type": "multipart/form-data"
        },
        timeout: 6e4
      }));
    },
    eM = (g, b) => (0, A._)(C.Ay.get({
      url: `/xapi/v1/${g}/query_meme_param2/task/${b}`
    })),
    eP = (g, b) => (0, A._)(C.Ay.delete$({
      url: `/xapi/v1/${g}/query_meme_param2/task/${b}`
    })),
    eR = g => (0, A._)(C.Ay.post({
      url: `/xapi/v1/${g}/openfour/templates`
    })),
    eB = g => (0, A._)(C.Ay.get({
      url: `/xapi/v1/${g}/flap/rwa_assets`
    })).then(g => g ?? []),
    ej = ["stock", "crypto", "etf", "private"],
    eO = ["curated", "custom"],
    eD = (g, b) => {
      let y = g[b];
      return "string" == typeof y && "" !== y ? y : void 0;
    },
    eL = g => {
      if ("object" != typeof g || null === g || Array.isArray(g)) return [];
      let b = g,
        y = eD(b, "address"),
        _ = eD(b, "symbol"),
        x = eD(b, "name");
      if (!y || !_ || !x) return [];
      let v = eD(b, "kind"),
        w = eD(b, "logo"),
        A = eD(b, "source");
      return [{
        address: y,
        symbol: _,
        name: x,
        ...(v && ej.includes(v) && {
          kind: v
        }),
        ...(w && {
          logo: w
        }),
        ...(A && eO.includes(A) && {
          source: A
        })
      }];
    },
    eF = async g => {
      let b = await (0, C.jJ)({
        url: `/xapi/v1/${g}/otc/reward_tokens`
      });
      if (!Array.isArray(b)) throw Error("Malformed OTC reward token list response");
      let y = new Set();
      return b.flatMap(eL).filter(({
        address: g
      }) => !y.has(g) && (y.add(g), !0));
    },
    eU = (g, b, y) => (0, A._)(C.Ay.post({
      url: `/xapi/v1/${g}/flap/dividend_token/check`,
      data: {
        raised_token: b,
        dividend_token: y
      }
    })),
    e$ = (g, b) => (0, A._)(C.Ay.get({
      url: `/xapi/v1/${g}/flap/quote_support`,
      data: {
        token: b
      }
    })),
    eW = "trench_v3",
    eq = "0x0000000000000000000000000000000000000000",
    eK = g => {
      if (!g || "object" != typeof g) return null;
      let {
        quote: b,
        symbol: y,
        decimals: _,
        category: x
      } = g;
      return "string" == typeof b && b && b.toLowerCase() !== eq && "string" == typeof y && y.trim() ? {
        symbol: y,
        address: b,
        decimals: "number" == typeof _ ? _ : 18,
        category: "string" == typeof x ? x : void 0
      } : null;
    },
    ez = g => (0, A._)(C.Ay.get({
      url: `/xapi/v1/${g}/trench_v3/quotes`
    })).then(g => {
      let b = g?.list;
      if (null == b) return [];
      if (!Array.isArray(b)) throw Error("trench v3 quotes: list is not an array");
      return b.map(eK).filter(g => null !== g);
    }),
    eV = g => "string" == typeof g ? g : void 0,
    eH = g => "number" == typeof g ? g : void 0,
    eG = g => "boolean" == typeof g ? g : void 0,
    eQ = g => g && "object" == typeof g ? g : void 0,
    eJ = g => {
      let b = eQ(g);
      if (!b) throw Error("trench v3 quote check: response is not an object");
      let y = eQ(b.onchain),
        _ = eQ(b.application);
      return {
        usable: !0 === b.usable,
        quote: eV(b.quote),
        symbol: eV(b.symbol),
        unusable_reason: eV(b.unusable_reason),
        onchain: y && {
          decimals: eH(y.decimals),
          symbol: eV(y.symbol),
          registered: eG(y.registered),
          active: eG(y.active)
        },
        application: _ && {
          state: eV(_.state),
          retry_after: eH(_.retry_after),
          updated_at: eH(_.updated_at)
        },
        application_checked: eG(b.application_checked)
      };
    },
    eY = (g, b) => (0, A._)(C.Ay.get({
      url: `/xapi/v1/${g}/trench_v3/quote/check`,
      data: {
        quote: b
      }
    })).then(eJ),
    eX = g => "number" == typeof g && Number.isFinite(g) && g >= 0 ? g : void 0,
    eZ = g => {
      let b = eQ(g);
      if (!b || Array.isArray(b)) throw Error("pump quote check: response is not an object");
      let y = b.tx_version;
      return {
        address: eV(b.address),
        launchable: !0 === b.launchable,
        reason: eV(b.reason),
        kind: eV(b.kind),
        symbol: eV(b.symbol),
        name: eV(b.name),
        decimals: eH(b.decimals),
        token_program: eV(b.token_program),
        depth: eH(b.depth),
        tx_version: "number" == typeof y || "string" == typeof y ? y : void 0,
        name_symbol_budget: eX(b.name_symbol_budget)
      };
    },
    e0 = (g, b) => (0, A._)(C.Ay.get({
      url: `/xapi/v1/${g}/pump/quote_check`,
      data: {
        address: b
      }
    })).then(eZ),
    e1 = (g, b) => (0, A._)(C.Ay.post({
      url: `/xapi/v1/${g}/trench_v3/quote/apply`,
      data: b
    })),
    e2 = (g, b) => b === eW ? ez(g) : (0, A._)(C.Ay.get({
      url: `/xapi/v1/${g}/${b}/quote_tokens`
    })).then(g => g ?? []);
  function e3(g, b) {
    return C.Ay.get({
      url: `/api/v1/token_banner/${g}/${b}`
    });
  }
  function e4(g, b) {
    return (0, A._)(e3(g, b));
  }
  function e6(g, b) {
    return C.Ay.get({
      url: `/api/v1/token_logo_history/${g}/${b}`,
      data: {
        chain: g,
        address: b
      }
    });
  }
  function e5(g) {
    let {
      chain: b,
      tokenAddress: y,
      ..._
    } = g;
    return C.Ay.get({
      data: _,
      url: `/vas/api/v1/dca_orders/${b}/${y}`,
      authType: w.AuthType.Access
    });
  }
  function e8(g) {
    return C.Ay.get({
      url: `/api/v1/token_lock_info/sol/${g}`
    });
  }
  let e7 = g => {
    let {
      chain: b,
      ...y
    } = g;
    return C.Ay.get({
      url: `/vas/api/v1/agged_token_transfers/${b}`,
      data: y
    });
  };
});
