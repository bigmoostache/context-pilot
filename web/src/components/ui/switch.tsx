import * as React from 'react'
import * as SwitchPrimitive from '@radix-ui/react-switch'
import { cn } from '@/lib/utils'

export function Switch({ className, ...props }: React.ComponentPropsWithoutRef<typeof SwitchPrimitive.Root>) {
  return (
    <SwitchPrimitive.Root
      className={cn(
        'peer inline-flex h-5 w-9 shrink-0 cursor-pointer items-center rounded-full border border-coal-600 bg-coal-700 transition-colors data-[state=checked]:bg-brass-500 data-[state=checked]:border-brass-500 focus-visible:outline-2 focus-visible:outline-brass-400',
        className,
      )}
      {...props}
    >
      <SwitchPrimitive.Thumb className="pointer-events-none block size-4 rounded-full bg-parchment-100 shadow transition-transform translate-x-0.5 data-[state=checked]:translate-x-[18px]" />
    </SwitchPrimitive.Root>
  )
}
