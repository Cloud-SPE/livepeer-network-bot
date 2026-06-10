#!/usr/bin/env bash
# Validate explorer API data against Livepeer onchain state (Arbitrum One).
#
# For each orchestrator this checks:
#   1. profile `total_stake`            vs BondingManager.transcoderTotalStake
#   2. each `/delegators` row           vs getDelegator (still bonded here?)
#                                       and pendingStake (current stake incl.
#                                       compounded rewards)
#   3. (--rewards FROM TO) leaderboard  vs sum/count of onchain Reward events
#
# Requires: cast (foundry), curl, python3.
#
# Usage:
#   ops/validate-onchain.sh                          # default fixture orchs
#   ops/validate-onchain.sh 0xabc... 0xdef...        # specific orchs
#   ops/validate-onchain.sh --rewards 2026-06-01 2026-06-07 0x141e...9683
set -euo pipefail

RPC="${ARBITRUM_RPC_URL:-https://arb1.arbitrum.io/rpc}"
API="${EXPLORER_BASE_URL:-https://livepeer-network-api.cloudspe.com}"
BONDING_MANAGER=0x35Bcf3c30594191d53231E4FF333E8A770453e40
ROUNDS_MANAGER=0xdd6f56DcC28D3F5f27084381fE8Df634985cc39f
REWARD_TOPIC=0x619caafabdd75649b302ba8419e48cccf64f37f1983ac4727cfb38b57703ffc9
STAKE_TOLERANCE_PCT=0.5

# Known orchestrators used as test fixtures (name:address).
DEFAULT_ORCHS=(
  "kilout:0xd603d6bf88aa061fcab8fa552026694a7fd005ce"
  "open-pool:0x5263e0ce3a97b634d8828ce4337ad0f70b30b077"
  "xodeapp:0xd00354656922168815fcd1e51cbddb9e359e3c7f"
  "moudi:0x141e6d4953b933746c770272126db2bd691a9683"
)

REWARDS_FROM="" REWARDS_TO=""
if [[ "${1:-}" == "--rewards" ]]; then
  REWARDS_FROM=$2 REWARDS_TO=$3
  shift 3
fi

ORCHS=("$@")
if [[ ${#ORCHS[@]} -eq 0 ]]; then
  ORCHS=("${DEFAULT_ORCHS[@]}")
fi

FAILURES=0
fail() { echo "  FAIL: $*"; FAILURES=$((FAILURES + 1)); }
pass() { echo "  ok:   $*"; }

wei_to_lpt() { python3 -c "print(int('$1') / 1e18)"; }

pct_diff() {
  python3 -c "
a, b = float('$1'), float('$2')
print(abs(a - b) / max(abs(b), 1e-18) * 100)
"
}

CURRENT_ROUND=$(cast call "$ROUNDS_MANAGER" 'currentRound()(uint256)' --rpc-url "$RPC")
echo "current round: $CURRENT_ROUND"

for entry in "${ORCHS[@]}"; do
  NAME=${entry%%:*}
  ADDR=${entry##*:}
  [[ "$NAME" == "$ADDR" ]] && NAME=$(echo "$ADDR" | cut -c1-10)
  ADDR=$(echo "$ADDR" | tr '[:upper:]' '[:lower:]')
  echo
  echo "== $NAME ($ADDR)"

  # 1. Profile total_stake vs transcoderTotalStake.
  ONCHAIN_STAKE_WEI=$(cast call "$BONDING_MANAGER" 'transcoderTotalStake(address)(uint256)' "$ADDR" --rpc-url "$RPC" | awk '{print $1}')
  ONCHAIN_STAKE=$(wei_to_lpt "$ONCHAIN_STAKE_WEI")
  API_STAKE=$(curl -fsS "$API/api/v1/orchestrators/$ADDR" | python3 -c "import json,sys; print(json.load(sys.stdin)['total_stake'])")
  DIFF=$(pct_diff "$API_STAKE" "$ONCHAIN_STAKE")
  if python3 -c "exit(0 if float('$DIFF') <= $STAKE_TOLERANCE_PCT else 1)"; then
    pass "total_stake api=$API_STAKE onchain=$ONCHAIN_STAKE (diff ${DIFF}%)"
  else
    fail "total_stake api=$API_STAKE onchain=$ONCHAIN_STAKE (diff ${DIFF}% > ${STAKE_TOLERANCE_PCT}%)"
  fi

  # 2. Delegator rows: membership + stake vs pendingStake.
  ROWS=$(curl -fsS "$API/api/v1/orchestrators/$ADDR/delegators?limit=10" | python3 -c "
import json, sys
for d in json.load(sys.stdin)['data']:
    print(d['delegator_address'], d['bonded_principal'])
")
  SHOWN_TOTAL=0
  while read -r DELEGATOR SHOWN; do
    [[ -z "$DELEGATOR" ]] && continue
    DELEGATE=$(cast call "$BONDING_MANAGER" 'getDelegator(address)(uint256,uint256,address,uint256,uint256,uint256,uint256)' "$DELEGATOR" --rpc-url "$RPC" | sed -n 3p | tr '[:upper:]' '[:lower:]')
    if [[ "$DELEGATE" != "$ADDR" ]]; then
      fail "delegator $DELEGATOR no longer bonded here (onchain delegate: $DELEGATE)"
      continue
    fi
    PENDING_WEI=$(cast call "$BONDING_MANAGER" 'pendingStake(address,uint256)(uint256)' "$DELEGATOR" "$CURRENT_ROUND" --rpc-url "$RPC" | awk '{print $1}')
    PENDING=$(wei_to_lpt "$PENDING_WEI")
    DIFF=$(pct_diff "$SHOWN" "$PENDING")
    if python3 -c "exit(0 if float('$DIFF') <= $STAKE_TOLERANCE_PCT else 1)"; then
      pass "delegator $DELEGATOR shown=$SHOWN pendingStake=$PENDING (diff ${DIFF}%)"
    else
      fail "delegator $DELEGATOR shown=$SHOWN pendingStake=$PENDING (diff ${DIFF}%)"
    fi
    SHOWN_TOTAL=$(python3 -c "print($SHOWN_TOTAL + float('$SHOWN'))")
  done <<<"$ROWS"

  # Invariant: any subset of delegators must fit inside the total stake.
  if python3 -c "exit(0 if $SHOWN_TOTAL <= float('$ONCHAIN_STAKE') else 1)"; then
    pass "top-10 sum $SHOWN_TOTAL <= transcoderTotalStake $ONCHAIN_STAKE"
  else
    fail "top-10 sum $SHOWN_TOTAL EXCEEDS transcoderTotalStake $ONCHAIN_STAKE"
  fi

  # 3. Rewards leaderboard vs onchain Reward events (optional, needs window).
  if [[ -n "$REWARDS_FROM" ]]; then
    FROM_TS=$(python3 -c "from datetime import datetime,timezone; print(int(datetime.fromisoformat('$REWARDS_FROM').replace(tzinfo=timezone.utc).timestamp()))")
    TO_TS=$(python3 -c "from datetime import datetime,timezone,timedelta; print(int((datetime.fromisoformat('$REWARDS_TO').replace(tzinfo=timezone.utc)+timedelta(days=1)).timestamp()))")
    FROM_BLOCK=$(cast find-block "$FROM_TS" --rpc-url "$RPC")
    TO_BLOCK=$(($(cast find-block "$TO_TS" --rpc-url "$RPC") - 1))
    TOPIC_ADDR=0x000000000000000000000000${ADDR#0x}
    ONCHAIN=$(cast logs --from-block "$FROM_BLOCK" --to-block "$TO_BLOCK" \
      --address "$BONDING_MANAGER" "$REWARD_TOPIC" "$TOPIC_ADDR" \
      --rpc-url "$RPC" --json | python3 -c "
import json, sys
logs = json.load(sys.stdin)
print(len(logs), sum(int(l['data'], 16) for l in logs) / 1e18)
")
    read -r ONCHAIN_COUNT ONCHAIN_LPT <<<"$ONCHAIN"
    API_REWARDS=$(curl -fsS "$API/api/v1/rewards/leaderboard?from=$REWARDS_FROM&to=$REWARDS_TO&limit=200" | python3 -c "
import json, sys
rows = [r for r in json.load(sys.stdin)['data'] if r['orchestrator_address'].lower() == '$ADDR']
print(rows[0]['reward_event_count'], rows[0]['sum_total_tokens']) if rows else print(0, 0)
")
    read -r API_COUNT API_LPT <<<"$API_REWARDS"
    DIFF=$(pct_diff "$API_LPT" "$ONCHAIN_LPT")
    if [[ "$API_COUNT" == "$ONCHAIN_COUNT" ]] && python3 -c "exit(0 if float('$DIFF') <= 0.01 else 1)"; then
      pass "rewards $REWARDS_FROM..$REWARDS_TO api=$API_COUNT/$API_LPT onchain=$ONCHAIN_COUNT/$ONCHAIN_LPT LPT"
    else
      fail "rewards $REWARDS_FROM..$REWARDS_TO api=$API_COUNT/$API_LPT onchain=$ONCHAIN_COUNT/$ONCHAIN_LPT LPT"
    fi
  fi
done

echo
if [[ $FAILURES -gt 0 ]]; then
  echo "$FAILURES check(s) FAILED"
  exit 1
fi
echo "all checks passed"
