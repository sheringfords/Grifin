# Scorecard (1–5; higher = better except K–O where higher = worse)

Qualitative argument in recommendation.md — scores expose tradeoffs only.

| Dim | W1 I/O-$ | W2 restore | F mig-gate | C stall-doc |
|---|---|---|---|---|
| A severity | 4 | 5 | 4 | 4 |
| B frequency | 5 | 2 | 4 | 3 |
| C evidence | 4 | 5 | 4 | 4 |
| D weak incumbents | 4 | 4 | 2 | 3 |
| E differentiation | 3 | 3 | 3 | 2 |
| F prototype feas. | 5 | 3 | 4 | 4 |
| G adoption | 5 | 3 | 4 | 4 |
| H demonstrability | 5 | 5 | 4 | 4 |
| I commercial | 4 | 3 | 3 | 2 |
| J durability | 4 | 4 | 4 | 3 |
| K saturation (inv) | 2 | 2 | 4 | 5 |
| L impl risk (inv) | 1 | 3 | 3 | 2 |
| M hw depend (inv) | 1 | 1 | 1 | 1 |
| N priv access (inv) | 1 | 2 | 1 | 1 |
| O incumbent (inv) | 3 | 2 | 4 | 4 |

Reading: W1 dominates frequency/feasibility/adoption/demonstrability with
the lowest implementation and privilege risk; its weaknesses are moat (E3)
and incumbent absorption (O3). W2 wins severity/evidence but pays frequency
(B2) and go-to-market (insurance sale). F is taxed by saturation (K4) and
incumbents owning CI (O4). C is taxed by research saturation (K5) and
"who pays for an explainer" (I2, O4).
