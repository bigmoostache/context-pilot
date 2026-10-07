# Social dynamics of LLM agent collectives, and what they imply for inter-thread collaboration

*T820 · v2 · research review. Replaces v1 (7587b416), which was mostly about the codebase.*

**Reading note.** Every LLM-collective result below comes from the paper's abstract, fetched through the arXiv API. The human results come from abstracts fetched through OpenAlex, except the textbook classics, which are marked as such. I don't quote any number that isn't in those sources. Each claim in §5 carries an evidence grade: **A** = replicated across independent groups or paradigms, **B** = one strong study, **C** = a single, model-only or abstract-level result.

---

## 0. Preliminaries: the system being studied

A Context Pilot agent process hosts several **threads**. Each thread is an LLM conversation with its own context, tasks and scratchpad, and it alternates between `MY_TURN` and `THEIR_TURN`. At most K = 2 threads run at the same time. Today the threads have no channel to each other. `Send` only posts to the calling thread (a8d821b1), and shared state (memories, logs, the entity DB, the filesystem) is written implicitly, with nobody coordinating it.

Three properties of this population matter for everything below:

1. **Homogeneity.** Every thread runs the same model with the same system prompt. In collective-intelligence terms, the population has close to zero cognitive diversity and highly correlated errors.
2. **Persistent shared memory.** Anything one thread writes to memories, logs or files stays there, and every other thread can read it later.
3. **A human hub.** One person talks to every thread, and the threads currently coordinate only through that person.

The design question is therefore not "which message API should we add?" It is: **which social dynamics should we induce or suppress in a small, homogeneous society of LLM agents that share persistent memory?**

## 1. Research questions

- **RQ1 (influence).** When does exposure to peers' outputs improve a group of LLM agents, and when does it degrade it?
- **RQ2 (pooling).** How do agents that each hold part of the evidence come to share it?
- **RQ3 (structure).** Which communication topologies and rhythms produce good collective outcomes, and does the answer depend on the task?
- **RQ4 (contagion).** How do errors, manipulation and injections spread through agent networks and persistent memory?
- **RQ5 (norms).** How do conventions, cooperation and collective biases emerge without anyone designing them?
- **RQ6 (organization).** When does coordination pay for its overhead, and which organizational forms make it pay?

---

## 2. Theoretical frame from human collective intelligence

### 2.1 Aggregation needs independence

The Condorcet jury theorem and the wisdom-of-crowds literature *(textbook)* both say that aggregating many judgments beats individuals only if their errors are at least partly independent. Two experiments define the modern debate:

- **Lorenz et al. 2011** (PNAS, N = 144): even mild social influence undermines the wisdom of crowds. Seeing others' estimates "narrows the diversity of opinions" *without improving collective error*.
- **Becker, Brackbill & Centola 2017** (PNAS): in **decentralized** networks, social influence "reliably improve[s]" crowd accuracy. The authors identify conditions under which influence, rather than independence, gives the best group judgment.

Read together: influence is neither good nor bad in itself. **The structure through which it flows decides the outcome.** Centralized influence amplifies whoever sits at the center. Decentralized influence lets accurate individuals pull the group without one node dominating.

### 2.2 Information pooling is biased toward what everyone already knows

**Stasser & Titus 1985** (JPSP) introduced the *hidden-profile* paradigm. Group discussion is "dominated by (a) information that members hold in common before discussion and (b) information that supports members' existent preferences". Groups therefore miss the decisive evidence that only one member holds. Pooling is the main reason to form a group, and it is also what groups do worst.

### 2.3 Exploration vs exploitation, set by topology and timing

- **Lazer & Friedman 2007** (ASQ, simulation): on complex problems, an efficient network "the better the short-run but the lower the long-run performance". An inefficient network "maintains diversity" and searches more thoroughly.
- **Mason & Watts 2012** (PNAS, 256 experiments, groups of 16) **contradict** this with real people: efficient networks *outperformed* inefficient ones even on a landscape built to favor inefficiency. They explain it through individual explore-or-exploit choices, which depend on both network position and payoffs.
- **Bernstein, Shore & Lazer 2018** (PNAS, traveling salesperson, groups of 3) move the question from *structure* to *time*. **Intermittent** social influence gave the high average of constant influence *and* the frequent optimal solutions of independent groups: "they learned from each other, while maintaining a high level of exploration."

So sparse topology is one lever for keeping diversity alive, and it isn't reliable on its own. Rhythm is another lever: work alone, then share, then work alone again. That lever has better evidence.

### 2.4 Diversity beats ability

**Hong & Page 2004** (PNAS, formal model): a random team drawn from a diverse pool outperforms a team of the best individual performers, because the best performers' "relatively greater ability is more than offset by their lack of problem-solving diversity". The result is debated in the literature, but the mechanism is uncontroversial. Agents that share heuristics get stuck at the same local optima.

### 2.5 What makes a group smart

**Woolley et al. 2010** (Science, 699 people in groups of 2-5) found a general collective-intelligence factor *c*. It is "not strongly correlated with the average or maximum individual intelligence" of the members. It correlates with their social sensitivity, with the **equality of conversational turn-taking**, and with the proportion of women in the group. The process matters more than the members' raw ability. Groups dominated by a few voices underperform.

### 2.6 Conventions tip at a critical mass

**Centola et al. 2018** (Science): once a committed minority reaches a critical mass, it "consistently" overturns an established convention. The size of that critical mass depends on the setting.

### 2.7 Classic group pathologies *(textbook)*

- **Asch** (1951/56): people conform to a unanimous wrong majority.
- **Janis** (1972), *groupthink*: cohesive groups suppress dissent and converge too early.
- **Wegner** (1987), *transactive memory*: groups perform better when members know who knows what.
- **Conway** (1968): a system's structure mirrors the communication structure of the organization that built it.

---

## 3. LLM agent collectives, organized by mechanism

### 3.1 Conformity and sycophancy: the default dynamic

- **MachineSoM** (Zhang et al., 2310.02124) builds four "societies" of agents with traits (easy-going vs overconfident) and thinking patterns (debate vs reflection). The agents show "conformity" and consensus-seeking that mirror human social psychology, and some collaboration strategies beat the state of the art with fewer tokens.
- **BenchForm** (Weng et al., 2501.13381, ICLR'25) is the first dedicated conformity benchmark. Conformity is real, and it grows with **interaction time** and **majority size**. Conforming agents produce rationalizations for their switch. Stronger personas and reflection reduce it.
- **"Talk isn't always cheap"** (Wynn et al., 2509.05396): debate can **lower** accuracy over rounds, *even when stronger models outnumber weaker ones*. Agents switch from correct to incorrect answers under peer reasoning and "favor agreement over challenging flawed reasoning".
- **"Peacemaker or troublemaker"** (Yao et al., 2509.23055) identifies inter-agent sycophancy as a core failure mode. It collapses disagreement too early and can push a debate below the single-agent baseline. The paper separates debater-driven from judge-driven sycophancy.

**Interpretation.** This is Asch, amplified. Human conformity is limited by private conviction and the social cost of capitulating. RLHF-tuned models are *trained* to accommodate their interlocutor, so a peer's message works like a user's push-back. Majority size and exposure time behave like the human variables, but the effect starts from a higher baseline.

### 3.2 Debate: decomposing the gains

| Study | Finding |
|---|---|
| Du et al. 2305.14325 | Multi-round debate improves math, strategic reasoning and factuality ("society of minds"). |
| Liang et al. 2305.19118 | Self-reflection suffers *Degeneration-of-Thought*: a confident model stops producing new ideas. Debate with "tit-for-tat" and a judge fixes it, but needs an adaptive stopping rule and only a *modest* level of contrarianism. A judge favors its own model family. |
| Smit et al. 2311.17371 | Debate does not reliably beat self-consistency or ensembling, and it is very sensitive to hyperparameters. Tuning the level of agreement helps. |
| Choi et al. 2508.17536 | Majority voting accounts for most of the gains attributed to debate. Debate behaves like a **martingale** over beliefs and gives no expected improvement in correctness. Only interventions biased toward correction help. |
| Li et al. 2402.05120 | Sampling plus voting scales with the number of agents, and the gain grows with task difficulty. |
| Chen et al. 2309.13007 (ReConcile) | A round table of *different* models with confidence-weighted voting improves results by up to 11.4%. Model diversity is critical. |
| Khan et al. 2402.06782 | Two expert models debating in front of a weaker non-expert judge: the judge reaches 76% (model judge) and 88% (human judge), against 48% and 60% without debate. More persuasive debaters lead to *more* truthful outcomes. |

**Interpretation.** Read with §2.1, the pattern is consistent. Most of what debate offers is **aggregation**, which needs independent samples. The talking itself adds variance but, on average, no truth (Choi). Debate earns its keep in two configurations:

1. the agents are actually **diverse**, through different models or different evidence (ReConcile, and the hidden-profile work in §3.3);
2. the structure is **adversarial with a judge** who can't solve the task alone. The asymmetry makes it cheaper to defend the truth than to attack it (Khan).

A homogeneous group talking in a symmetric peer chat is the worst case.

### 3.3 Information pooling: hidden profiles in LLM groups

- **Consilience** (Babu et al., 2608.20564) studies hidden-profile tasks where each agent holds part of the evidence. Fixed schedules, round-robin and unstructured debate offer no guarantee of pooling. A controller tracks a compact state (**uncertainty, disagreement, evidence gain, redundancy, premature consensus**) and chooses both the next intervention (challenge, clarify, seek evidence, route) and the next speaker. Across 12 models it beats fixed and unstructured protocols, and it *sometimes beats the full-information baseline*. **How communication is controlled can matter more than how much information each agent has.**
- **Rational groupthink** (Harel et al., 1412.7172, theory): long-lived rational agents who observe each other's *actions* learn more slowly than just four agents who exchange their private *signals*. The agents herd and ignore their own evidence for long stretches.

**Interpretation.** The LLM results repeat Stasser & Titus in another substrate, and Harel provides the mechanism. **Passing on conclusions destroys information; passing on evidence keeps it.** A thread that sends "the bug is in X" invites herding. A thread that sends "here is trace Y and observation Z" contributes a signal the receiver can weigh.

### 3.4 Conventions and collective bias

**Ashery, Aiello & Baronchelli** (2410.08948, *Science Advances* 2025) ran naming games in decentralized LLM populations. Universal conventions emerge spontaneously. **A strong collective bias appears even though no individual agent is biased**, and committed adversarial minorities can flip an established convention once they reach a critical mass. This reproduces Centola 2018 (§2.6) with LLMs.

**Interpretation.** Collective bias is a property of the interaction, not of the agents, so auditing agents one at a time won't detect it. Shared memory can act as a *committed minority*: a memory slot written once and read by every thread behaves like a member who never changes its mind.

### 3.5 Cooperation and the commons

- **Vallinder & Hughes** (2412.10270) ran a donor game with indirect reciprocity over several generations. Societies of Claude 3.5 Sonnet agents became far more cooperative than Gemini 1.5 Flash societies, which in turn beat GPT-4o. Costly punishment helped only the Claude societies, and results varied strongly with the random seed. **Cooperation is a cultural trait that depends on the model**, and it is fragile.
- **GovSim** (Piatti et al., 2404.16698): in shared-resource dilemmas, every model except the strongest depleted the commons, and the best survival rate stayed under 54%. Communication was critical. Failures came from an inability to reason about the long-term equilibrium of the group. Prompting "universalization" reasoning ("what if everyone did this?") helped.

**Interpretation.** For us, the commons are the K = 2 concurrency slots, the human's attention, shared memory slots (220, fixed) and the token budget. Threads that act only for themselves will overgraze all four, unless the norm is spelled out or the system enforces it.

### 3.6 Opinion dynamics

**Chuang et al.** (2311.09618): networks of LLM agents converge toward scientifically accurate beliefs (a strong built-in bias toward accuracy). Inducing **confirmation bias** produces opinion **fragmentation**, as classic agent-based models predict.

**Interpretation.** Left alone, LLM collectives drift toward consensus *with their shared prior*. That helps when the prior is right and is dangerous when it's wrong, because a homogeneous fleet has no internal source of dissent. Breaking out of it requires deliberately injected heterogeneity, not more conversation.

### 3.7 Contagion: errors, manipulation and injection

| Study | Spread mechanism | Key result |
|---|---|---|
| Lee & Tiwari 2410.07283 (*Prompt Infection*) | Self-replicating prompt injection passed from LLM to LLM | Spreads like a virus, even when agents don't share messages publicly. Defense: *LLM Tagging*, i.e. labeling where each piece of content came from. |
| Gu et al. 2402.08567 (*Agent Smith*) | One adversarial image, then pairwise chats | Spread exponentially until almost all of 1M agents were infected. The authors state a containment principle, but no practical defense yet. |
| Ju et al. 2407.07791 | Manipulated knowledge made more persuasive | Spreads through benign agents and **persists in their retrieval memory after the interaction ends**. They suggest guardian agents and fact-checking. |
| Huang et al. 2408.00989 | Faulty agents | Hierarchical A→(B↔C) is the most resilient structure: -5.5%, against -10.5% and -23.7% for the others. A *Challenger* role and an *Inspector* role recover up to 96.4% of errors. |
| Zhang et al. 2410.02506 (*AgentPrune*) | Redundant edges | Pruning the communication graph *also* improves robustness under two adversarial attacks (+3.5-10.8%). |

**Interpretation.** LLMs copy perfectly. A human retelling a rumor distorts it, and the distortion acts as a natural brake; agents pass an injection on intact. Persistent shared memory turns a transient infection into a chronic one (Ju). Every inter-agent edge is an attack surface, and every shared store can host it. The defenses that work belong to the **structure**, not the content: tracking provenance, pruning edges, hierarchical verification, and dedicated challenger roles.

### 3.8 Topology and scale

- **Sparse debate** (Li et al., 2406.11776): sparse topologies match or beat a full mesh on reasoning and factuality, at a much lower cost.
- **AgentPrune** (2410.02506) formally defines *communication redundancy*. One-shot pruning reaches comparable accuracy at $5.6 vs $43.7 and cuts 28.1-72.8% of tokens.
- **GTD** (2510.07799), task-adaptive graphs: static chains, stars and complete graphs either waste tokens or become bottlenecks. Sparse adaptive graphs score highest at the lowest cost. With one faulty agent, GTD loses 0.3 points, a complete graph 2 points and DyLAN 13 points. Gains saturate at about 4 agents.
- **MacNet** (Qian et al., 2406.07155): directed acyclic graphs of more than 1,000 agents. *Irregular* topologies beat regular ones, and quality follows a **logistic** "collaborative scaling law".
- **Science of scaling agent systems** (Kim et al., 2512.08296): 260 configurations, 6 benchmarks, 5 architectures, 3 model families. Coordination shows **diminishing returns once the single-agent baseline is high** ("capability saturation"). Tool-heavy tasks pay a large overhead, and architectures without centralized verification propagate more errors. The effect ranges from **+80.8%** on decomposable financial reasoning to **-70.0%** on sequential planning. Their model predicts the best architecture for 87% of held-out configurations.
- **Anthropic's research system** (Jun 2025, engineering report): an orchestrator plus 3-5 parallel workers scored +90.2% over a single agent on the internal evaluation. Multi-agent runs used about 15 times the tokens of chat. Coding parallelizes less well. Early failures included spawning too many workers and agents distracting each other with constant updates.

**Interpretation.** The LLM topology results repeat the Lazer vs Mason & Watts debate. Sparse beats dense *on average*, but the dominant variable is the **task's structure**: decomposable or sequential, tool-heavy or not, and how strong the single agent already is. Topology design should be adaptive and conditioned on the task, not fixed in advance.

### 3.9 Organization, roles and failure modes

- **MetaGPT** (2308.00352): standard operating procedures written into prompts, assembly-line roles and intermediate verification reduce the cascading hallucinations seen in chat-based multi-agent systems.
- **AgentVerse** (2308.10848): a dynamically assembled group beats a single agent, and emergent social behaviors appear, some helpful and some harmful.
- **Generative Agents** (Park et al., 2304.03442): among 25 agents, information (party invitations) spreads and coordination emerges from memory, reflection and planning, with no explicit protocol.
- **Theory of mind** (Li et al., 2310.10701): LLM agents show high-order theory of mind but lose track over long horizons and hallucinate the state of the task. Giving them an **explicit belief-state representation** improves both task performance and theory of mind.
- **Blackboard** (Salemi et al., 2510.01285): a coordinator posts requests, helpers **volunteer**, and replies go privately to the poster. It beats assignment by the coordinator by 13-57% relative, without the coordinator needing a model of each helper's competence.
- **MAST** (Cemri et al., 2503.13657): 1,600+ traces from 7 frameworks, 14 failure modes in 3 categories, kappa 0.88. The categories: system design 44%, inter-agent misalignment 32%, verification 24%. The most frequent modes are step repetition, not knowing when to stop, reasoning that doesn't match actions, and missing verification. Standard message formats don't fix misalignment; agents fail to model what the other agent needs to know.

**Interpretation.** These systems succeed by making **state explicit**: standard procedures, belief states, boards. They fail where state stays implicit in chat: termination, task drift, unverified handoffs. This is transactive memory (§2.7) built as a data structure. An agent that can't model what its peer knows needs that knowledge written down.

---

## 4. Where LLM collectives differ from human ones

| Property | Humans | LLM agents | Consequence |
|---|---|---|---|
| Error correlation | Moderate (different lives and training) | Very high within one model family | The Condorcet benefit shrinks. Diversity has to be engineered: models, evidence, roles, temperature. |
| Conformity baseline | Bounded by conviction and social cost | Raised by RLHF accommodation | Peer messages work like user push-back (§3.1). |
| Copy fidelity | Lossy retelling | Exact | Injections and errors spread without decaying (§3.7). |
| Memory | Personal, decays | Shared, persistent, retrieved | One bad write becomes a permanent committed minority (§3.4, §3.7). |
| Turn-taking | Dominated by a few unless managed | Whatever the scheduler allows | Woolley's turn-taking equality becomes a scheduling parameter. |
| Fatigue and stakes | Present | Absent | Endless ping-pong is possible; termination has to be imposed (MAST). |
| Model of others | Rich, implicit | Fragile over long horizons | Belief state must be explicit (§3.9). |

---

## 5. Synthesis: eight propositions

**P1. The default dynamic of a homogeneous LLM group is premature consensus.** *(A)* BenchForm, "Talk isn't always cheap", "Peacemaker", Asch, Janis. Group interaction has to be designed to resist it, not to encourage it.

**P2. Exchange evidence, not verdicts.** *(A)* Stasser & Titus, Harel, Consilience, Lorenz. Messages that carry observations, traces and references keep independent signal. Messages that carry conclusions trigger herding.

**P3. Most of the value of group talk is aggregation over independent samples.** *(B)* Choi (martingale), Smit, Li "More agents", Condorcet. Without independence, more talk adds cost and conformity but no accuracy.

**P4. Diversity is the precondition for any gain.** *(A)* Hong & Page, ReConcile, Woolley, Chuang. In a single-model fleet, diversity has to come from the inputs: different evidence, different roles (challenger, inspector), different context.

**P5. Rhythm matters more than wiring.** *(B)* Bernstein 2018 directly; the Lazer vs Mason & Watts conflict and the sparse-topology results point the same way. Alternate independent work with brief, scheduled exchanges, rather than leaving channels permanently open.

**P6. Whatever can spread, will spread, and persistent memory makes it chronic.** *(A)* Prompt Infection, Agent Smith, Ju, AgentPrune. Every edge and every shared store needs provenance, and verification has to be part of the structure.

**P7. Coordination pays only when the task decomposes and the single agent isn't already saturated.** *(B)* Kim (+80.8% to -70.0%), Anthropic's report on coding, GTD's saturation at about 4 agents. The default should be no coordination, with coordination opened per task.

**P8. Explicit shared state beats implicit chat.** *(A)* MetaGPT, ToM belief states, the blackboard, MAST, transactive memory. Termination, ownership, open questions and "who knows what" should be data, not inferred from chat.

---

## 6. Testable hypotheses for our fleet

Each hypothesis can be checked on recorded multi-thread sessions or on a small harness (2-4 threads, coding and research tasks).

- **H1 (P1, P2).** On a hidden-profile coding task (each thread gets different logs), "evidence-only" messages beat free-form messages on root-cause accuracy. *Metric:* accuracy, and the fraction of unshared clues that are ever mentioned.
- **H2 (P1).** The rate at which a thread switches its answer after reading a peer increases with the number of peers that agree, as BenchForm predicts. *Metric:* how often a thread flips from correct to incorrect after a message.
- **H3 (P5).** Exchanges at scheduled checkpoints beat continuously open channels on the best solution found, at equal tokens.
- **H4 (P6).** An injection planted in one thread's tool output reaches other threads through shared memory unless provenance tags are present. *Metric:* how far it spreads, and how long it persists.
- **H5 (P7).** On sequential tasks (one refactor along a call chain), two cooperating threads do worse than one thread. On decomposable tasks (independent modules), they do better.
- **H6 (P4).** Assigning a *challenger* role to one thread lowers the correct-to-incorrect flip rate compared with symmetric peers.

**Protocol.** Use fixed task seeds and ≥ 5 seeds per condition (Vallinder showed strong seed sensitivity). Report the single-thread baseline with the same total tokens, as Kim and Smit recommend. Log every message with its provenance so that diffusion can be reconstructed afterwards.

---

## 7. Design implications (brief)

Each item lists the propositions it follows from.

1. **Default: no channel.** Collaboration opens per task, when the task decomposes (P7).
2. **Evidence-typed messages.** Messages carry observations and file references, and a bare verdict is discouraged by the tool's schema (P2).
3. **A board with volunteering and private replies**, not group rooms. Replies don't influence each other, so independence is kept until aggregation (P1, P3, blackboard).
4. **Checkpoint rhythm.** Peers exchange at scheduled points rather than through live chat (P5).
5. **Provenance on every inter-thread item and shared memory write**, so a receiver can weigh and trace it (P6).
6. **Explicit state:** owner, termination condition, open questions and a who-knows-what registry (P8).
7. **Asymmetric roles** (requester and challenger, or worker and inspector) instead of symmetric peers (P1, P4, Huang, Khan).
8. **Commons limits:** caps on open requests per thread and messages per exchange, and a universalization line in the prompt (GovSim).

## 8. Threats to validity

- The LLM sources were read at **abstract level**. Effect sizes and conditions may be narrower than stated here.
- Most benchmarks are QA or math. **Coding agents with tools and persistent memory are under-studied.** Kim and the Anthropic report are the closest.
- Results depend strongly on the model (Vallinder) and the seed. Conclusions drawn from GPT-4o or Gemini may not carry over to Claude, our model.
- Several human anchors are themselves contested, notably Hong & Page and Lazer vs Mason & Watts. I use them for mechanisms, not for exact effect sizes.
- Our population (2-4 threads, one human hub) is far smaller than most simulations (25 to 1M agents). Dynamics at scale, such as tipping points and exponential contagion, apply here only through shared memory, which effectively makes the population larger over time.

## References

**Human collective intelligence.** Asch 1951/56 *(textbook)* · Becker, Brackbill & Centola 2017 PNAS · Bernstein, Shore & Lazer 2018 PNAS · Centola et al. 2018 Science · Conway 1968 *(textbook)* · Hong & Page 2004 PNAS · Janis 1972 *(textbook)* · Lazer & Friedman 2007 ASQ · Lorenz et al. 2011 PNAS · Mason & Watts 2012 PNAS · Stasser & Titus 1985 JPSP · Wegner 1987 *(textbook)* · Woolley et al. 2010 Science.

**LLM collectives (arXiv).** Conformity: 2310.02124 MachineSoM · 2501.13381 BenchForm · 2509.05396 Talk isn't always cheap · 2509.23055 Peacemaker or troublemaker. Debate: 2305.14325 Du · 2305.19118 Liang (MAD) · 2311.17371 Smit · 2508.17536 Debate or vote · 2402.06782 Khan · 2309.13007 ReConcile · 2402.05120 More agents. Pooling: 2608.20564 Consilience · 1412.7172 Rational groupthink. Norms: 2410.08948 Ashery · 2412.10270 Vallinder · 2404.16698 GovSim · 2311.09618 Chuang. Contagion: 2410.07283 Prompt Infection · 2402.08567 Agent Smith · 2407.07791 Ju · 2408.00989 Huang. Topology and scale: 2406.11776 Sparse debate · 2410.02506 AgentPrune · 2510.07799 GTD · 2406.07155 MacNet · 2512.08296 Kim. Organization: 2308.00352 MetaGPT · 2308.10848 AgentVerse · 2304.03442 Generative Agents · 2310.10701 ToM · 2510.01285 Blackboard · 2503.13657 MAST · 2501.06322 survey. Anthropic, "How we built our multi-agent research system" (Jun 2025).
