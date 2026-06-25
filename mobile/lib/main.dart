import 'package:dio/dio.dart';
import 'package:flutter/material.dart';

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

void main() {
  runApp(const ContextPilotApp());
}

// ---------------------------------------------------------------------------
// App root
// ---------------------------------------------------------------------------

class ContextPilotApp extends StatelessWidget {
  const ContextPilotApp({super.key});

  @override
  Widget build(BuildContext context) => MaterialApp(
        title: 'Context Pilot',
        debugShowCheckedModeBanner: false,
        theme: ThemeData(
          colorSchemeSeed: const Color(0xFF6750A4),
          useMaterial3: true,
          brightness: Brightness.dark,
        ),
        home: const FleetScreen(),
      );
}

// ---------------------------------------------------------------------------
// Fleet screen — lists agents from the orchestrator
// ---------------------------------------------------------------------------

class FleetScreen extends StatefulWidget {
  const FleetScreen({super.key});

  @override
  State<FleetScreen> createState() => _FleetScreenState();
}

class _FleetScreenState extends State<FleetScreen> {
  final TextEditingController _urlController = TextEditingController(
    text: 'http://127.0.0.1:7878',
  );

  List<AgentSummary> _agents = [];
  bool _loading = false;
  String? _error;

  Future<void> _fetchFleet() async {
    setState(() {
      _loading = true;
      _error = null;
    });

    try {
      final dio = Dio(BaseOptions(
        connectTimeout: const Duration(seconds: 5),
        receiveTimeout: const Duration(seconds: 5),
      ));
      final response = await dio.get<Map<String, dynamic>>(
        '${_urlController.text}/api/fleet',
      );

      final data = response.data?['data'] as Map<String, dynamic>? ?? {};
      final agents = <AgentSummary>[];

      for (final entry in data.entries) {
        final agent = entry.value as Map<String, dynamic>;
        final cost = agent['cost'] as Map<String, dynamic>? ?? {};
        final roster = agent['roster'] as List<dynamic>? ?? [];

        agents.add(AgentSummary(
          id: entry.key,
          lifecycle: agent['lifecycle'] as String? ?? 'unknown',
          phase: agent['phase'] as String? ?? 'unknown',
          costUsd: (cost['cost_usd'] as num?)?.toDouble() ?? 0.0,
          threadCount: roster.length,
          threads: roster
              .map((t) => ThreadInfo(
                    name: (t as Map<String, dynamic>)['name'] as String? ?? '?',
                    status: t['status'] as String? ?? '?',
                  ))
              .toList(),
        ));
      }

      setState(() {
        _agents = agents;
        _loading = false;
      });
    } on DioException catch (e) {
      setState(() {
        _error = e.message ?? 'Connection failed';
        _loading = false;
      });
    }
  }

  @override
  void initState() {
    super.initState();
    _fetchFleet();
  }

  @override
  void dispose() {
    _urlController.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => Scaffold(
      appBar: AppBar(
        title: const Text('Context Pilot'),
        centerTitle: true,
      ),
      body: Column(
        children: [
          // --- URL bar ---
          Padding(
            padding: const EdgeInsets.all(12),
            child: Row(
              children: [
                Expanded(
                  child: TextField(
                    controller: _urlController,
                    decoration: const InputDecoration(
                      labelText: 'Backend URL',
                      border: OutlineInputBorder(),
                      isDense: true,
                    ),
                    onSubmitted: (_) => _fetchFleet(),
                  ),
                ),
                const SizedBox(width: 8),
                FilledButton.icon(
                  onPressed: _loading ? null : _fetchFleet,
                  icon: const Icon(Icons.refresh),
                  label: const Text('Fetch'),
                ),
              ],
            ),
          ),

          // --- Content ---
          Expanded(child: _buildContent()),
        ],
      ),
    );

  Widget _buildContent() {
    if (_loading) {
      return const Center(child: CircularProgressIndicator());
    }
    if (_error != null) {
      return Center(
        child: Padding(
          padding: const EdgeInsets.all(24),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              const Icon(Icons.error_outline, size: 48, color: Colors.red),
              const SizedBox(height: 12),
              Text(
                _error!,
                textAlign: TextAlign.center,
                style: Theme.of(context).textTheme.bodyLarge,
              ),
            ],
          ),
        ),
      );
    }
    if (_agents.isEmpty) {
      return const Center(
        child: Text('No agents found.', style: TextStyle(fontSize: 16)),
      );
    }

    return RefreshIndicator(
      onRefresh: _fetchFleet,
      child: ListView.builder(
        padding: const EdgeInsets.symmetric(horizontal: 12),
        itemCount: _agents.length,
        itemBuilder: (context, index) => _AgentCard(agent: _agents[index]),
      ),
    );
  }
}

// ---------------------------------------------------------------------------
// Agent card widget
// ---------------------------------------------------------------------------

class _AgentCard extends StatelessWidget {
  const _AgentCard({required this.agent});

  final AgentSummary agent;

  Color _lifecycleColor() => switch (agent.lifecycle) {
      'running' => Colors.green,
      'stopped' => Colors.red,
      _ => Colors.orange,
    };

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);

    return Card(
      margin: const EdgeInsets.only(bottom: 12),
      child: Padding(
        padding: const EdgeInsets.all(16),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            // --- Header: ID + status chip ---
            Row(
              children: [
                Expanded(
                  child: Text(
                    agent.id.substring(0, 8),
                    style: theme.textTheme.titleMedium?.copyWith(
                      fontFamily: 'monospace',
                      fontWeight: FontWeight.bold,
                    ),
                  ),
                ),
                Chip(
                  label: Text(agent.lifecycle),
                  backgroundColor: _lifecycleColor().withValues(alpha: 0.2),
                  side: BorderSide(color: _lifecycleColor()),
                  labelStyle: TextStyle(color: _lifecycleColor()),
                ),
              ],
            ),
            const SizedBox(height: 8),

            // --- Info row ---
            Row(
              children: [
                _InfoPill(
                  icon: Icons.play_arrow,
                  label: agent.phase,
                ),
                const SizedBox(width: 8),
                _InfoPill(
                  icon: Icons.attach_money,
                  label: '\$${agent.costUsd.toStringAsFixed(2)}',
                ),
                const SizedBox(width: 8),
                _InfoPill(
                  icon: Icons.forum,
                  label: '${agent.threadCount} threads',
                ),
              ],
            ),

            // --- Thread list ---
            if (agent.threads.isNotEmpty) ...[
              const SizedBox(height: 12),
              const Divider(height: 1),
              const SizedBox(height: 8),
              ...agent.threads.map(
                (t) => Padding(
                  padding: const EdgeInsets.symmetric(vertical: 2),
                  child: Row(
                    children: [
                      Icon(
                        t.status == 'my_turn'
                            ? Icons.circle
                            : Icons.circle_outlined,
                        size: 10,
                        color: t.status == 'my_turn'
                            ? Colors.amber
                            : Colors.grey,
                      ),
                      const SizedBox(width: 8),
                      Expanded(
                        child: Text(
                          t.name,
                          style: theme.textTheme.bodySmall,
                          overflow: TextOverflow.ellipsis,
                        ),
                      ),
                      Text(
                        t.status.replaceAll('_', ' '),
                        style: theme.textTheme.labelSmall?.copyWith(
                          color: theme.colorScheme.onSurfaceVariant,
                        ),
                      ),
                    ],
                  ),
                ),
              ),
            ],
          ],
        ),
      ),
    );
  }
}

// ---------------------------------------------------------------------------
// Small info pill widget
// ---------------------------------------------------------------------------

class _InfoPill extends StatelessWidget {
  const _InfoPill({required this.icon, required this.label});

  final IconData icon;
  final String label;

  @override
  Widget build(BuildContext context) => Container(
      padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 4),
      decoration: BoxDecoration(
        color: Theme.of(context).colorScheme.surfaceContainerHighest,
        borderRadius: BorderRadius.circular(12),
      ),
      child: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          Icon(icon, size: 14),
          const SizedBox(width: 4),
          Text(label, style: Theme.of(context).textTheme.labelSmall),
        ],
      ),
    );
}

// ---------------------------------------------------------------------------
// Data models
// ---------------------------------------------------------------------------

class AgentSummary {
  const AgentSummary({
    required this.id,
    required this.lifecycle,
    required this.phase,
    required this.costUsd,
    required this.threadCount,
    required this.threads,
  });

  final String id;
  final String lifecycle;
  final String phase;
  final double costUsd;
  final int threadCount;
  final List<ThreadInfo> threads;
}

class ThreadInfo {
  const ThreadInfo({required this.name, required this.status});

  final String name;
  final String status;
}
